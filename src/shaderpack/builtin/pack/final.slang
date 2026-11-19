// Torch Vibrant: exposure and a filmic curve, to the display.

#include "lib/common.slang"

struct Varyings {
    float4 clip : SV_Position;
    float2 uv : TEXCOORD0;
};

Varyings vertex(IrisVertexIn input) {
    Varyings o;
    o.clip = iris_screen(input);
    o.uv = iris_attributes(input).uv;
    return o;
}

float3 aces(float3 x) {
    float a = 2.51;
    float b = 0.03;
    float c = 2.43;
    float d = 0.59;
    float e = 0.14;
    return clamp((x * (a * x + b)) / (x * (c * x + d) + e), 0.0, 1.0);
}

float4 fragment(Varyings v) : SV_Target0 {
    float3 c = colortex0.SampleLevel(colortex0_sampler, v.uv, 0.0).rgb;
    // The eye opens up at night and underground.
    float open = clamp(float(frame.eyeBrightnessSmooth.y) / 240.0, 0.0, 1.0);
    float bright = daylight() * open;
    c *= EXPOSURE * lerp(1.7, 0.8, bright);
    c = aces(c);
    float3 grey = float3(luma(c));
    c = clamp(lerp(grey, c, 1.12), 0.0, 1.0);
    return iris_output(float4(pow(c, 1.0 / 2.2), 1.0));
}
