#[cfg(target_os = "windows")]
mod platform {
    use std::{ffi::c_void, ptr};

    use ffmpeg_the_third::{
        codec::{self, encoder},
        ffi,
        format::{self, flag::Flags, Pixel},
        frame, packet,
        software::scaling::{self, Flags as ScaleFlags},
        Dictionary, Rational,
    };

    use crate::capture::models::RawFrame;
    use crate::encoder::{
        audio_capture::AudioCaptureService,
        config::{EncoderConfig, QualityMode, VideoCodec, VideoEncoderPreference},
        output_paths::prepare_output_paths,
        video_encoder_status::set_live_video_encoder_label,
    };

    enum VideoInputPipeline {
        Cpu {
            scaler: scaling::Context,
            src_frame: frame::Video,
            dst_frame: frame::Video,
        },
        GpuTextureD3d11,
    }

    struct EncoderContext {
        output_ctx: format::context::Output,
        video_enc: encoder::Video,
        input_pipeline: VideoInputPipeline,
        stream_idx: usize,
        time_base: Rational,
        first_timestamp_ms: Option<u64>,
        last_pts: i64,
    }

    pub struct FfmpegEncoderConsumer {
        config: EncoderConfig,
        ctx: Option<EncoderContext>,
        audio_capture: Option<AudioCaptureService>,
    }

    #[derive(Debug, Clone, Copy, Default)]
    pub struct VideoEncoderCapabilities {
        pub nvenc: bool,
        pub amf: bool,
        pub qsv: bool,
        pub software: bool,
    }

    // FFmpeg mantiene estado interno no thread-safe; este consumer se usa con exclusión mutua.
    unsafe impl Send for FfmpegEncoderConsumer {}

    impl FfmpegEncoderConsumer {
        pub fn new(mut config: EncoderConfig) -> Result<Self, String> {
            config.validate()?;
            ffmpeg_the_third::init()
                .map_err(|err| format!("No se pudo inicializar FFmpeg: {err}"))?;
            set_live_video_encoder_label(None);

            let final_output_path = config.output_path.clone();
            let prepared_paths = prepare_output_paths(final_output_path.clone())?;
            config.output_path = prepared_paths.temp_output_path.clone();

            let audio_capture = AudioCaptureService::new(
                config.audio.clone(),
                config.format.clone(),
                config.quality_mode.clone(),
                config.output_path.clone(),
                final_output_path,
                prepared_paths.temp_dir,
            );

            Ok(Self {
                config,
                ctx: None,
                audio_capture: Some(audio_capture),
            })
        }

        pub fn on_frame(&mut self, frame: RawFrame) -> Result<(), String> {
            if !frame.is_valid() {
                return Ok(());
            }

            if self.ctx.is_none() {
                self.initialize(&frame)?;
            }

            self.encode_frame(frame)
        }

        pub fn on_stop(&mut self) -> Result<(), String> {
            self.finalize()
        }

        fn initialize(&mut self, frame: &RawFrame) -> Result<(), String> {
            let frame_width = frame.width;
            let frame_height = frame.height;
            let gpu_surface_only = frame.has_gpu_texture() && !frame.has_cpu_data();

            let (codec_kind, allow_fallback) = match &self.config.codec {
                Some(codec) => (codec.clone(), false),
                None => (self.config.format.default_codec(), true),
            };

            let (mut out_w, mut out_h) =
                self.config.resolution.dimensions(frame_width, frame_height);
            if out_w % 2 == 1 {
                out_w = out_w.saturating_sub(1);
            }
            if out_h % 2 == 1 {
                out_h = out_h.saturating_sub(1);
            }
            if out_w < 2 || out_h < 2 {
                return Err(
                    "La resolución resultante es demasiado pequeña (mínimo 2x2)".to_string()
                );
            }

            let path_str =
                self.config.output_path.to_str().ok_or_else(|| {
                    "La ruta de salida contiene caracteres no válidos".to_string()
                })?;

            let mut output_ctx =
                format::output_as(path_str, self.config.format.ffmpeg_format_name()).map_err(
                    |err| format!("No se pudo crear el archivo de salida '{path_str}': {err}"),
                )?;

            let needs_global_header = output_ctx.format().flags().contains(Flags::GLOBAL_HEADER);
            let time_base = Rational::new(1, 1_000);
            let candidates = encoder_candidates(
                &codec_kind,
                allow_fallback,
                &self.config.video_encoder_preference,
                gpu_surface_only,
            );
            if candidates.is_empty() {
                return Err(format!(
                    "No hay encoders compatibles para el modo de entrada {} con codec {:?}",
                    if gpu_surface_only { "GPU" } else { "CPU" },
                    codec_kind
                ));
            }

            let mut selected_encoder_name: Option<&'static str> = None;
            let mut selected_codec = None;
            let mut selected_video_enc: Option<encoder::Video> = None;
            let mut open_failures = Vec::<String>::new();

            for name in &candidates {
                let Some(candidate_codec) = encoder::find_by_name(name) else {
                    continue;
                };

                let (encoder_opts, has_custom_opts) =
                    self.build_encoder_options(name, &codec_kind, out_w, out_h);

                let mut open_attempt =
                    |opts: Dictionary| -> Result<encoder::Video, ffmpeg_the_third::Error> {
                        let mut candidate_enc =
                            codec::context::Context::new_with_codec(candidate_codec)
                                .encoder()
                                .video()
                                .map_err(|err| {
                                    open_failures
                                        .push(format!("{name}: no se pudo crear contexto ({err})"));
                                    err
                                })?;

                        candidate_enc.set_width(out_w);
                        candidate_enc.set_height(out_h);
                        candidate_enc.set_format(if gpu_surface_only {
                            Pixel::D3D11
                        } else {
                            Pixel::YUV420P
                        });
                        candidate_enc.set_time_base(time_base);
                        candidate_enc
                            .set_frame_rate(Some(Rational::new(self.config.fps as i32, 1)));

                        if needs_global_header {
                            candidate_enc.set_flags(codec::Flags::GLOBAL_HEADER);
                        }

                        candidate_enc.open_with(opts)
                    };

                match open_attempt(encoder_opts) {
                    Ok(opened) => {
                        selected_encoder_name = Some(*name);
                        selected_codec = Some(candidate_codec);
                        selected_video_enc = Some(opened);
                        break;
                    }
                    Err(err) => {
                        if has_custom_opts {
                            match open_attempt(Dictionary::new()) {
                                Ok(opened) => {
                                    selected_encoder_name = Some(*name);
                                    selected_codec = Some(candidate_codec);
                                    selected_video_enc = Some(opened);
                                    break;
                                }
                                Err(fallback_err) => open_failures.push(format!(
                                    "{name}: {err} | fallback sin opciones: {fallback_err}"
                                )),
                            }
                        } else {
                            open_failures.push(format!("{name}: {err}"));
                        }
                    }
                }
            }

            let encoder_name = selected_encoder_name.ok_or_else(|| {
                let details = if open_failures.is_empty() {
                    String::new()
                } else {
                    format!(" Detalles: {}", open_failures.join(" | "))
                };

                format!(
                    "No se pudo abrir un encoder compatible para {}. Probados: {}.{}",
                    codec_kind.ffmpeg_encoder_name(),
                    candidates.join(", "),
                    details
                )
            })?;

            let found_codec = selected_codec.expect("codec seleccionado ausente");
            let video_enc = selected_video_enc.expect("encoder seleccionado ausente");
            let backend_label = selected_backend_label(encoder_name);
            if gpu_surface_only && backend_label == "CPU" {
                return Err(
                    "El modo GPU de textura D3D11 requiere un encoder de hardware (NVENC/AMF/QSV)"
                        .to_string(),
                );
            }

            let live_codec_label = selected_codec_label(&codec_kind);
            set_live_video_encoder_label(Some(format!("{backend_label} / {live_codec_label}")));

            let mut stream = output_ctx
                .add_stream(found_codec)
                .map_err(|err| format!("No se pudo agregar el stream de video: {err}"))?;
            let stream_idx = stream.index();

            stream.copy_parameters_from_context(&video_enc);
            stream.set_time_base(time_base);
            stream.set_rate(Rational::new(self.config.fps as i32, 1));
            stream.set_avg_frame_rate(Rational::new(self.config.fps as i32, 1));

            output_ctx
                .write_header()
                .map_err(|err| format!("No se pudo escribir cabecera del contenedor: {err}"))?;

            let input_pipeline = if gpu_surface_only {
                VideoInputPipeline::GpuTextureD3d11
            } else {
                let scale_flags = match self.config.quality_mode {
                    QualityMode::Performance => ScaleFlags::FAST_BILINEAR,
                    QualityMode::Balanced => ScaleFlags::BILINEAR,
                    QualityMode::Quality => ScaleFlags::BICUBIC,
                };

                let scaler = scaling::Context::get(
                    Pixel::BGRA,
                    frame_width,
                    frame_height,
                    Pixel::YUV420P,
                    out_w,
                    out_h,
                    scale_flags,
                )
                .map_err(|err| format!("No se pudo crear el escalador de color: {err}"))?;
                let src_frame = frame::Video::new(Pixel::BGRA, frame_width, frame_height);
                let dst_frame = frame::Video::new(Pixel::YUV420P, out_w, out_h);

                VideoInputPipeline::Cpu {
                    scaler,
                    src_frame,
                    dst_frame,
                }
            };

            self.ctx = Some(EncoderContext {
                output_ctx,
                video_enc,
                input_pipeline,
                stream_idx,
                time_base,
                first_timestamp_ms: None,
                last_pts: -1,
            });

            self.audio_capture
                .as_mut()
                .ok_or_else(|| "AudioCaptureService no disponible".to_string())?
                .start()?;

            Ok(())
        }

        fn build_encoder_options(
            &self,
            encoder_name: &str,
            codec: &VideoCodec,
            out_w: u32,
            out_h: u32,
        ) -> (Dictionary<'_>, bool) {
            let mut options = Dictionary::new();
            let mut has_options = false;
            let gop = recommended_gop_frames(self.config.fps);
            let target_kbps = estimate_target_bitrate_kbps(
                out_w,
                out_h,
                self.config.fps,
                codec,
                &self.config.quality_mode,
            );
            let maxrate_kbps = target_kbps.saturating_mul(match self.config.quality_mode {
                QualityMode::Performance => 100,
                QualityMode::Balanced => 125,
                QualityMode::Quality => 140,
            }) / 100;
            let bufsize_kbps = target_kbps.saturating_mul(match self.config.quality_mode {
                QualityMode::Performance => 50,
                QualityMode::Balanced => 100,
                QualityMode::Quality => 130,
            }) / 100;

            match codec {
                VideoCodec::H264 | VideoCodec::H265 => {
                    if encoder_name.contains("nvenc") {
                        let preset = match self.config.quality_mode {
                            QualityMode::Performance => "p3",
                            QualityMode::Balanced => "p5",
                            QualityMode::Quality => "p6",
                        };

                        let nvenc_cq = match self.config.quality_mode {
                            QualityMode::Performance => self.config.crf.saturating_add(5).min(36),
                            QualityMode::Balanced => self.config.crf.min(32),
                            QualityMode::Quality => self.config.crf.saturating_sub(2).max(14),
                        };
                        let tune = match self.config.quality_mode {
                            QualityMode::Performance => "ull",
                            QualityMode::Balanced => "ll",
                            QualityMode::Quality => "hq",
                        };
                        let use_cbr = matches!(self.config.quality_mode, QualityMode::Performance);

                        options.set("preset", preset);
                        options.set("rc", if use_cbr { "cbr" } else { "vbr" });
                        if !use_cbr {
                            options.set("cq", &nvenc_cq.to_string());
                        }
                        options.set("b:v", &format!("{target_kbps}k"));
                        options.set("maxrate", &format!("{maxrate_kbps}k"));
                        options.set("bufsize", &format!("{bufsize_kbps}k"));
                        options.set("g", &gop.to_string());
                        options.set("bf", "0");
                        options.set("rc-lookahead", "0");
                        options.set("tune", tune);
                        if matches!(self.config.quality_mode, QualityMode::Quality) {
                            options.set("spatial_aq", "1");
                            options.set("temporal_aq", "1");
                            options.set("aq-strength", "8");
                        } else {
                            options.set("spatial_aq", "0");
                            options.set("temporal_aq", "0");
                        }
                        has_options = true;
                    }

                    if encoder_name.starts_with("libx26") {
                        options.set("crf", &self.config.crf.to_string());
                        options.set("preset", self.config.preset.as_str());
                        options.set("tune", "zerolatency");
                        has_options = true;
                    }

                    if encoder_name.contains("_amf") {
                        let quality = match self.config.quality_mode {
                            QualityMode::Performance => "speed",
                            QualityMode::Balanced => "balanced",
                            QualityMode::Quality => "quality",
                        };
                        let usage = match self.config.quality_mode {
                            QualityMode::Performance => "ultralowlatency",
                            QualityMode::Balanced => "lowlatency",
                            QualityMode::Quality => "transcoding",
                        };
                        options.set("quality", quality);
                        options.set("usage", usage);
                        options.set("rc", "cbr");
                        options.set("b:v", &format!("{target_kbps}k"));
                        options.set("maxrate", &format!("{maxrate_kbps}k"));
                        options.set("bufsize", &format!("{bufsize_kbps}k"));
                        options.set("g", &gop.to_string());
                        options.set("bf", "0");
                        has_options = true;
                    }

                    if encoder_name.contains("_qsv")
                        && matches!(self.config.quality_mode, QualityMode::Performance)
                    {
                        options.set("low_power", "1");
                        options.set("bf", "0");
                        options.set("async_depth", "1");
                        options.set("g", &gop.to_string());
                        has_options = true;
                    } else if encoder_name.contains("_qsv") {
                        let qsv_quality = self.config.crf.min(40);
                        options.set("global_quality", &qsv_quality.to_string());
                        options.set("bf", "0");
                        options.set("async_depth", "1");
                        options.set("g", &gop.to_string());
                        has_options = true;
                    }
                }
                VideoCodec::Vp9 => {
                    if encoder_name.contains("vp9") {
                        options.set("crf", &self.config.crf.to_string());
                        options.set("b", "0");
                        options.set("deadline", "realtime");
                        options.set("cpu-used", "8");
                        has_options = true;
                    }
                }
            }

            (options, has_options)
        }

        fn encode_frame(&mut self, frame: RawFrame) -> Result<(), String> {
            let ctx = self
                .ctx
                .as_mut()
                .ok_or_else(|| "El encoder no fue inicializado".to_string())?;

            match &mut ctx.input_pipeline {
                VideoInputPipeline::Cpu {
                    scaler,
                    src_frame,
                    dst_frame,
                } => {
                    if !frame.has_cpu_data() || !frame.is_cpu_layout_valid() {
                        return Err("Frame inválido para pipeline CPU (BGRA)".to_string());
                    }

                    let row_bytes = (frame.width.saturating_mul(4)) as usize;
                    let src_stride = frame.row_stride_bytes as usize;
                    let dst_stride = src_frame.stride(0);
                    let dst_data = src_frame.data_mut(0);

                    let rows = frame.height as usize;
                    let min_input_size = rows.saturating_mul(src_stride);
                    if frame.data.len() < min_input_size {
                        return Err(format!(
                            "Buffer de frame incompleto: {} < {}",
                            frame.data.len(),
                            min_input_size
                        ));
                    }

                    let contiguous_copy_size = rows.saturating_mul(row_bytes);
                    if src_stride == row_bytes
                        && dst_stride == row_bytes
                        && contiguous_copy_size <= dst_data.len()
                    {
                        dst_data[..contiguous_copy_size]
                            .copy_from_slice(&frame.data[..contiguous_copy_size]);
                    } else {
                        for row_idx in 0..rows {
                            let src_offset = row_idx.saturating_mul(src_stride);
                            let dst_offset = row_idx * dst_stride;
                            if dst_offset + row_bytes > dst_data.len() {
                                break;
                            }
                            let src_slice = &frame.data[src_offset..src_offset + row_bytes];
                            dst_data[dst_offset..dst_offset + row_bytes].copy_from_slice(src_slice);
                        }
                    }

                    scaler
                        .run(src_frame, dst_frame)
                        .map_err(|err| format!("Error en conversión de color: {err}"))?;

                    let first_ts = *ctx.first_timestamp_ms.get_or_insert(frame.timestamp_ms);
                    let rel_ts_ms = frame.timestamp_ms.saturating_sub(first_ts) as i64;
                    let pts = if rel_ts_ms <= ctx.last_pts {
                        ctx.last_pts + 1
                    } else {
                        rel_ts_ms
                    };
                    dst_frame.set_pts(Some(pts));
                    ctx.last_pts = pts;

                    ctx.video_enc
                        .send_frame(dst_frame)
                        .map_err(|err| format!("Error enviando frame al encoder: {err}"))?;
                }
                VideoInputPipeline::GpuTextureD3d11 => {
                    Self::encode_gpu_texture_frame(ctx, frame)?;
                }
            }

            self.drain_packets()
        }

        fn encode_gpu_texture_frame(
            ctx: &mut EncoderContext,
            mut frame: RawFrame,
        ) -> Result<(), String> {
            let texture_ptr = frame
                .take_gpu_texture_ptr()
                .ok_or_else(|| "Frame GPU recibido sin textura D3D11".to_string())?;

            let mut hw_frame = frame::Video::empty();
            hw_frame.set_format(Pixel::D3D11);
            hw_frame.set_width(frame.width);
            hw_frame.set_height(frame.height);

            let first_ts = *ctx.first_timestamp_ms.get_or_insert(frame.timestamp_ms);
            let rel_ts_ms = frame.timestamp_ms.saturating_sub(first_ts) as i64;
            let pts = if rel_ts_ms <= ctx.last_pts {
                ctx.last_pts + 1
            } else {
                rel_ts_ms
            };
            hw_frame.set_pts(Some(pts));
            ctx.last_pts = pts;

            unsafe {
                let av_frame = hw_frame.as_mut_ptr();

                (*av_frame).data[0] = texture_ptr as *mut u8;
                (*av_frame).data[1] = ptr::null_mut();

                let texture_buf = ffi::av_buffer_create(
                    texture_ptr as *mut u8,
                    1,
                    Some(release_d3d11_texture_buffer),
                    texture_ptr as *mut c_void,
                    0,
                );
                if texture_buf.is_null() {
                    release_d3d11_texture_buffer(
                        texture_ptr as *mut c_void,
                        texture_ptr as *mut u8,
                    );
                    return Err(
                        "No se pudo crear AVBufferRef para textura D3D11 del frame".to_string()
                    );
                }
                (*av_frame).buf[0] = texture_buf;
            }

            ctx.video_enc
                .send_frame(&hw_frame)
                .map_err(|err| format!("Error enviando frame GPU al encoder: {err}"))?;

            Ok(())
        }

        fn drain_packets(&mut self) -> Result<(), String> {
            let ctx = self
                .ctx
                .as_mut()
                .ok_or_else(|| "El encoder no fue inicializado".to_string())?;

            let mut encoded_packet = packet::Packet::empty();
            while ctx.video_enc.receive_packet(&mut encoded_packet).is_ok() {
                encoded_packet.set_stream(ctx.stream_idx);

                let stream = ctx.output_ctx.stream(ctx.stream_idx).ok_or_else(|| {
                    format!(
                        "No se encontró stream de salida para índice {}",
                        ctx.stream_idx
                    )
                })?;
                encoded_packet.rescale_ts(ctx.time_base, stream.time_base());

                encoded_packet
                    .write_interleaved(&mut ctx.output_ctx)
                    .map_err(|err| format!("Error escribiendo packet en contenedor: {err}"))?;
            }

            Ok(())
        }

        fn finalize(&mut self) -> Result<(), String> {
            let mut video_error: Option<String> = None;

            if self.ctx.is_some() {
                let send_eof_result = self
                    .ctx
                    .as_mut()
                    .expect("contexto de encoder ausente")
                    .video_enc
                    .send_eof();

                if let Err(err) = send_eof_result {
                    video_error = Some(format!("Error enviando EOF al encoder: {err}"));
                } else if let Err(err) = self.drain_packets() {
                    video_error = Some(err);
                } else if let Err(err) = self
                    .ctx
                    .as_mut()
                    .expect("contexto de encoder ausente")
                    .output_ctx
                    .write_trailer()
                {
                    video_error = Some(format!(
                        "Error escribiendo trailer del contenedor: {err}. El archivo puede quedar corrupto."
                    ));
                }
            }

            self.ctx = None;

            if let Some(audio_capture) = self.audio_capture.take() {
                audio_capture.finalize_and_mux_detached();
            }

            set_live_video_encoder_label(None);

            if let Some(err) = video_error {
                return Err(err);
            }

            Ok(())
        }
    }

    fn encoder_candidates(
        codec: &VideoCodec,
        allow_fallback: bool,
        preference: &VideoEncoderPreference,
        gpu_surface_only: bool,
    ) -> Vec<&'static str> {
        let push_unique = |list: &mut Vec<&'static str>, candidate: &'static str| {
            if !list.contains(&candidate) {
                list.push(candidate);
            }
        };

        match codec {
            VideoCodec::H264 => {
                let mut list = Vec::new();
                match preference {
                    VideoEncoderPreference::Nvenc => {
                        push_unique(&mut list, "h264_nvenc");
                        push_unique(&mut list, "h264_amf");
                        push_unique(&mut list, "h264_qsv");
                    }
                    VideoEncoderPreference::Amf => {
                        push_unique(&mut list, "h264_amf");
                        push_unique(&mut list, "h264_nvenc");
                        push_unique(&mut list, "h264_qsv");
                    }
                    VideoEncoderPreference::Qsv => {
                        push_unique(&mut list, "h264_qsv");
                        push_unique(&mut list, "h264_nvenc");
                        push_unique(&mut list, "h264_amf");
                    }
                    VideoEncoderPreference::Software => {}
                    VideoEncoderPreference::Auto => {
                        push_unique(&mut list, "h264_nvenc");
                        push_unique(&mut list, "h264_amf");
                        push_unique(&mut list, "h264_qsv");
                    }
                }
                if !gpu_surface_only {
                    push_unique(&mut list, "libx264");
                    push_unique(&mut list, "h264");
                    if allow_fallback {
                        push_unique(&mut list, "mpeg4");
                    }
                }
                list
            }
            VideoCodec::H265 => {
                let mut list = Vec::new();
                match preference {
                    VideoEncoderPreference::Nvenc => {
                        push_unique(&mut list, "hevc_nvenc");
                        push_unique(&mut list, "hevc_amf");
                        push_unique(&mut list, "hevc_qsv");
                    }
                    VideoEncoderPreference::Amf => {
                        push_unique(&mut list, "hevc_amf");
                        push_unique(&mut list, "hevc_nvenc");
                        push_unique(&mut list, "hevc_qsv");
                    }
                    VideoEncoderPreference::Qsv => {
                        push_unique(&mut list, "hevc_qsv");
                        push_unique(&mut list, "hevc_nvenc");
                        push_unique(&mut list, "hevc_amf");
                    }
                    VideoEncoderPreference::Software => {}
                    VideoEncoderPreference::Auto => {
                        push_unique(&mut list, "hevc_nvenc");
                        push_unique(&mut list, "hevc_amf");
                        push_unique(&mut list, "hevc_qsv");
                    }
                }
                if !gpu_surface_only {
                    push_unique(&mut list, "libx265");
                    push_unique(&mut list, "hevc");
                }
                list
            }
            VideoCodec::Vp9 => vec!["libvpx-vp9", "vp9"],
        }
    }

    unsafe extern "C" fn release_d3d11_texture_buffer(opaque: *mut c_void, _data: *mut u8) {
        use windows::{core::Interface, Win32::Graphics::Direct3D11::ID3D11Texture2D};

        if opaque.is_null() {
            return;
        }

        let _ = ID3D11Texture2D::from_raw(opaque as *mut _);
    }

    fn recommended_gop_frames(fps: u32) -> u32 {
        let safe_fps = fps.clamp(1, 240);
        safe_fps.saturating_mul(2).clamp(30, 300)
    }

    fn estimate_target_bitrate_kbps(
        width: u32,
        height: u32,
        fps: u32,
        codec: &VideoCodec,
        quality_mode: &QualityMode,
    ) -> u32 {
        let bpp = match quality_mode {
            QualityMode::Performance => 0.055_f64,
            QualityMode::Balanced => 0.075_f64,
            QualityMode::Quality => 0.1_f64,
        };
        let codec_factor = match codec {
            VideoCodec::H264 => 1.0_f64,
            VideoCodec::H265 => 0.72_f64,
            VideoCodec::Vp9 => 0.68_f64,
        };

        let pixels_per_sec = f64::from(width) * f64::from(height) * f64::from(fps.clamp(1, 240));
        let estimated_kbps = (pixels_per_sec * bpp * codec_factor / 1_000.0).round();
        let clamped = estimated_kbps.clamp(2_500.0, 80_000.0);
        clamped as u32
    }

    fn selected_backend_label(encoder_name: &str) -> &'static str {
        if encoder_name.contains("nvenc") {
            "NVENC"
        } else if encoder_name.contains("_amf") {
            "AMF"
        } else if encoder_name.contains("_qsv") {
            "QSV"
        } else {
            "CPU"
        }
    }

    fn selected_codec_label(codec: &VideoCodec) -> &'static str {
        match codec {
            VideoCodec::H264 => "H.264",
            VideoCodec::H265 => "H.265",
            VideoCodec::Vp9 => "VP9",
        }
    }

    fn can_open_encoder(encoder_name: &str) -> bool {
        let Some(codec) = encoder::find_by_name(encoder_name) else {
            return false;
        };

        let mut enc = match codec::context::Context::new_with_codec(codec)
            .encoder()
            .video()
        {
            Ok(enc) => enc,
            Err(_) => return false,
        };

        enc.set_width(1280);
        enc.set_height(720);
        enc.set_format(Pixel::YUV420P);
        enc.set_time_base(Rational::new(1, 1_000));
        enc.set_frame_rate(Some(Rational::new(30, 1)));

        enc.open_with(Dictionary::new()).is_ok()
    }

    pub fn detect_video_encoder_capabilities() -> VideoEncoderCapabilities {
        let _ = ffmpeg_the_third::init();

        VideoEncoderCapabilities {
            nvenc: can_open_encoder("h264_nvenc"),
            amf: can_open_encoder("h264_amf"),
            qsv: can_open_encoder("h264_qsv"),
            software: can_open_encoder("libx264") || can_open_encoder("h264"),
        }
    }
}

#[cfg(not(target_os = "windows"))]
mod platform {
    use crate::capture::models::RawFrame;
    use crate::encoder::config::EncoderConfig;

    #[derive(Debug, Clone, Copy, Default)]
    pub struct VideoEncoderCapabilities {
        pub nvenc: bool,
        pub amf: bool,
        pub qsv: bool,
        pub software: bool,
    }

    pub struct FfmpegEncoderConsumer;

    impl FfmpegEncoderConsumer {
        pub fn new(_config: EncoderConfig) -> Result<Self, String> {
            Err("El encoder FFmpeg solo está disponible para Windows".to_string())
        }

        pub fn on_frame(&mut self, _frame: RawFrame) -> Result<(), String> {
            Ok(())
        }

        pub fn on_stop(&mut self) -> Result<(), String> {
            Ok(())
        }
    }

    pub fn detect_video_encoder_capabilities() -> VideoEncoderCapabilities {
        VideoEncoderCapabilities {
            nvenc: false,
            amf: false,
            qsv: false,
            software: false,
        }
    }
}

pub use platform::{detect_video_encoder_capabilities, FfmpegEncoderConsumer};
