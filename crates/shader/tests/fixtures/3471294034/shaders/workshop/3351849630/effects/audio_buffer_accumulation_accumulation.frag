// Minimal persisted workshop 3471294034 reduction for
// workshop/3351849630/effects/audio_buffer_accumulation_accumulation.

#define RESOLUTION 32

varying vec4 v_TexCoord;
varying vec3 v_AccumulationRate;

uniform sampler2D g_Texture1;
uniform float u_VolumeScale;
uniform float g_AudioSpectrum32Left[32];
uniform float g_AudioSpectrum32Right[32];

void main() {
    vec4 pastAlbedo = texSample2DLod(g_Texture1, v_TexCoord.xy, 0.0);
    vec4 albedo;

    int index = floor(v_TexCoord.x * RESOLUTION);
    vec2 audio = vec2(g_AudioSpectrum32Left[index], g_AudioSpectrum32Right[index]);
    audio *= u_VolumeScale;

    vec2 pastAudio = pastAlbedo.xy + pastAlbedo.zw;
    vec2 rate = v_AccumulationRate.y * step(audio * 2, pastAudio) +
        v_AccumulationRate.x * step(pastAudio, audio * 2);
    albedo.xy = mix(pastAlbedo.xy, min(audio * 2, 1), rate);
    albedo.zw = mix(pastAlbedo.zw, max(audio * 2 - 1, 0), rate);

    gl_FragColor = albedo;
}
