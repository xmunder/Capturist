#[cfg(target_os = "windows")]
mod platform {
    use ffmpeg_the_third::{
        codec::{self, encoder},
        format::{self, flag::Flags, Pixel},
        frame, packet,
        software::scaling::{self, Flags as ScaleFlags},
        Dictionary, Rational,
    };

    use crate::capture::models::RawFrame;
    use crate::encoder::config::{EncoderConfig, QualityMode, VideoCodec, VideoEncoderPreference};
    use crate::encoder::video_encoder_status::set_live_video_encoder_label;

    struct EncoderContext {
        output_ctx: format::context::Output,
        video_enc: encoder::Video,
        scaler: scaling::Context,
        stream_idx: usize,
        time_base: Rational,
        first_timestamp_ms: Option<u64>,
        last_pts: i64,
        src_frame: frame::Video,
        dst_frame: frame::Video,
    }

    pub struct FfmpegEncoderConsumer {
        config: EncoderConfig,
        ctx: Option<EncoderContext>,
    }

    // FFmpeg mantiene estado interno no thread-safe; este consumer se usa con exclusión mutua.
    unsafe impl Send for FfmpegEncoderConsumer {}

    impl FfmpegEncoderConsumer {
        pub fn new(config: EncoderConfig) -> Result<Self, String> {
            config.validate()?;
            ffmpeg_the_third::init()
                .map_err(|err| format!("No se pudo inicializar FFmpeg: {err}"))?;
            set_live_video_encoder_label(None);

            Ok(Self { config, ctx: None })
        }

        pub fn on_frame(&mut self, frame: RawFrame) -> Result<(), String> {
            if !frame.is_valid() {
                return Ok(());
            }

            if self.ctx.is_none() {
                self.initialize(frame.width, frame.height)?;
            }

            self.encode_frame(frame)
        }

        pub fn on_stop(&mut self) -> Result<(), String> {
            self.finalize()
        }

        fn initialize(&mut self, frame_width: u32, frame_height: u32) -> Result<(), String> {
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
            );

            let mut selected_encoder_name: Option<&'static str> = None;
            let mut selected_codec = None;
            let mut selected_video_enc: Option<encoder::Video> = None;
            let mut open_failures = Vec::<String>::new();

            for name in &candidates {
                let Some(candidate_codec) = encoder::find_by_name(name) else {
                    continue;
                };

                let (encoder_opts, has_custom_opts) = self.build_encoder_options(name, &codec_kind);

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
                        candidate_enc.set_format(Pixel::YUV420P);
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

            set_live_video_encoder_label(Some(selected_backend_label(encoder_name).to_string()));

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

            self.ctx = Some(EncoderContext {
                output_ctx,
                video_enc,
                scaler,
                stream_idx,
                time_base,
                first_timestamp_ms: None,
                last_pts: -1,
                src_frame,
                dst_frame,
            });

            Ok(())
        }

        fn build_encoder_options(
            &self,
            encoder_name: &str,
            codec: &VideoCodec,
        ) -> (Dictionary<'_>, bool) {
            let mut options = Dictionary::new();
            let mut has_options = false;

            match codec {
                VideoCodec::H264 | VideoCodec::H265 => {
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
                        options.set("quality", quality);
                        has_options = true;
                    }

                    if encoder_name.contains("_qsv")
                        && matches!(self.config.quality_mode, QualityMode::Performance)
                    {
                        options.set("low_power", "1");
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

            let src_frame = &mut ctx.src_frame;
            let dst_frame = &mut ctx.dst_frame;

            let stride = (frame.width * 4) as usize;
            let dst_stride = src_frame.stride(0);
            let dst_data = src_frame.data_mut(0);
            if dst_stride == stride && frame.data.len() <= dst_data.len() {
                dst_data[..frame.data.len()].copy_from_slice(&frame.data);
            } else {
                for (row_idx, row_chunk) in frame.data.chunks(stride).enumerate() {
                    let dst_offset = row_idx * dst_stride;
                    if dst_offset + row_chunk.len() <= dst_data.len() {
                        dst_data[dst_offset..dst_offset + row_chunk.len()]
                            .copy_from_slice(row_chunk);
                    }
                }
            }

            ctx.scaler
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

            self.drain_packets()
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
                push_unique(&mut list, "libx264");
                push_unique(&mut list, "h264");
                if allow_fallback {
                    push_unique(&mut list, "mpeg4");
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
                push_unique(&mut list, "libx265");
                push_unique(&mut list, "hevc");
                list
            }
            VideoCodec::Vp9 => vec!["libvpx-vp9", "vp9"],
        }
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
}

#[cfg(not(target_os = "windows"))]
mod platform {
    use crate::capture::models::RawFrame;
    use crate::encoder::config::EncoderConfig;

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
}

pub use platform::FfmpegEncoderConsumer;
