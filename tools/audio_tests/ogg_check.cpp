// Decodes each Ogg Vorbis given with the port's decoder (externals/stb/stb_vorbis.c, as src/Audio/PortSound.cpp does)
// and prints its channels, rate, length and peak: a file the port would not play shows here first.
//
//   ogg_check.exe FILE.ogg ...
#include <stb_vorbis.c>

#include <cmath>
#include <cstdio>
#include <cstdlib>

int main(int argc, char** argv) {
    int bad = 0;
    for (int i = 1; i < argc; ++i) {
        int channels = 0, rate = 0;
        short* out = nullptr;
        const int frames = stb_vorbis_decode_filename(argv[i], &channels, &rate, &out);
        if (frames <= 0 || !out) {
            std::printf("%s: NOT DECODED (%d)\n", argv[i], frames);
            ++bad;
            continue;
        }
        int peak = 0;
        for (long long s = 0; s < 1LL * frames * channels; ++s) peak = std::abs(out[s]) > peak ? std::abs(out[s]) : peak;
        std::printf("%s: %d ch, %d Hz, %.2f s, peak %.1f dBFS\n", argv[i], channels, rate, double(frames) / rate,
                    peak ? 20.0 * std::log10(peak / 32768.0) : -180.0);
        std::free(out);
    }
    return bad;
}
