#ifndef AVCODEC_AVFFT_H
#define AVCODEC_AVFFT_H

/*
 * Compatibility shim for ffmpeg-sys-the-third bindgen:
 * ffmpeg 7.x removed libavcodec/avfft.h, but the crate still requests it
 * for pre-8.0 lavc versions. We delegate to libavutil/tx.h where FFT/transform
 * APIs live in modern FFmpeg.
 */
#include <libavutil/tx.h>

#endif /* AVCODEC_AVFFT_H */
