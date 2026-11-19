// `frame`: the pass's uniform block, read from its bindless buffer. Each
// member's byte offset is a specialization constant the engine sets from
// the program's uniform layout (`pipeline::UniformLayout`), so a program
// pays only for what it reads. The GLSL type after each member is the
// engine's for that uniform (`uniforms::builtin_type`); a test holds them
// equal.

[SpecializationConstant] const uint IRIS_SLOT_frame = 0;

ByteAddressBuffer iris_frame_buffer() { return iris_buffer(IRIS_SLOT_frame); }
float iris_load_float(uint at) { return asfloat(iris_frame_buffer().Load(at)); }
int iris_load_int(uint at) { return asint(iris_frame_buffer().Load(at)); }
float2 iris_load_vec2(uint at) { return asfloat(iris_frame_buffer().Load2(at)); }
float3 iris_load_vec3(uint at) { return asfloat(iris_frame_buffer().Load3(at)); }
float4 iris_load_vec4(uint at) { return asfloat(iris_frame_buffer().Load4(at)); }
int2 iris_load_ivec2(uint at) { return asint(iris_frame_buffer().Load2(at)); }
int3 iris_load_ivec3(uint at) { return asint(iris_frame_buffer().Load3(at)); }
// Stored by columns; the constructor takes rows.
float4x4 iris_load_mat4(uint at) {
    ByteAddressBuffer b = iris_frame_buffer();
    return transpose(float4x4(
        asfloat(b.Load4(at)),
        asfloat(b.Load4(at + 16)),
        asfloat(b.Load4(at + 32)),
        asfloat(b.Load4(at + 48))));
}

[SpecializationConstant] const uint IRIS_OFF_iris_ModelViewMatrix = 0;
[SpecializationConstant] const uint IRIS_OFF_iris_ProjectionMatrix = 0;
[SpecializationConstant] const uint IRIS_OFF_iris_ModelViewProjectionMatrix = 0;
[SpecializationConstant] const uint IRIS_OFF_iris_CameraPosition = 0;
[SpecializationConstant] const uint IRIS_OFF_iris_ScreenSize = 0;
[SpecializationConstant] const uint IRIS_OFF_iris_Time = 0;
[SpecializationConstant] const uint IRIS_OFF_iris_ModelViewMatrixInverse = 0;
[SpecializationConstant] const uint IRIS_OFF_iris_ProjectionMatrixInverse = 0;
[SpecializationConstant] const uint IRIS_OFF_iris_FogColor = 0;
[SpecializationConstant] const uint IRIS_OFF_iris_FogParams = 0;
[SpecializationConstant] const uint IRIS_OFF_cameraPosition = 0;
[SpecializationConstant] const uint IRIS_OFF_cameraPositionFract = 0;
[SpecializationConstant] const uint IRIS_OFF_cameraPositionInt = 0;
[SpecializationConstant] const uint IRIS_OFF_previousCameraPosition = 0;
[SpecializationConstant] const uint IRIS_OFF_eyeBrightnessSmooth = 0;
[SpecializationConstant] const uint IRIS_OFF_far = 0;
[SpecializationConstant] const uint IRIS_OFF_near = 0;
[SpecializationConstant] const uint IRIS_OFF_fogColor = 0;
[SpecializationConstant] const uint IRIS_OFF_frameCounter = 0;
[SpecializationConstant] const uint IRIS_OFF_frameTimeCounter = 0;
[SpecializationConstant] const uint IRIS_OFF_gbufferModelView = 0;
[SpecializationConstant] const uint IRIS_OFF_gbufferModelViewInverse = 0;
[SpecializationConstant] const uint IRIS_OFF_gbufferProjection = 0;
[SpecializationConstant] const uint IRIS_OFF_gbufferProjectionInverse = 0;
[SpecializationConstant] const uint IRIS_OFF_gbufferPreviousModelView = 0;
[SpecializationConstant] const uint IRIS_OFF_gbufferPreviousProjection = 0;
[SpecializationConstant] const uint IRIS_OFF_hasSkylight = 0;
[SpecializationConstant] const uint IRIS_OFF_isEyeInWater = 0;
[SpecializationConstant] const uint IRIS_OFF_rainStrength = 0;
[SpecializationConstant] const uint IRIS_OFF_shadowLightPosition = 0;
[SpecializationConstant] const uint IRIS_OFF_shadowModelView = 0;
[SpecializationConstant] const uint IRIS_OFF_shadowProjection = 0;
[SpecializationConstant] const uint IRIS_OFF_sunPosition = 0;
[SpecializationConstant] const uint IRIS_OFF_torchShadowBlend = 0;
[SpecializationConstant] const uint IRIS_OFF_torchShadowModelViewPrevious = 0;

struct IrisFrame {
    property float4x4 iris_ModelViewMatrix { get { return iris_load_mat4(IRIS_OFF_iris_ModelViewMatrix); } } // mat4
    property float4x4 iris_ProjectionMatrix { get { return iris_load_mat4(IRIS_OFF_iris_ProjectionMatrix); } } // mat4
    property float4x4 iris_ModelViewProjectionMatrix { get { return iris_load_mat4(IRIS_OFF_iris_ModelViewProjectionMatrix); } } // mat4
    property float4 iris_CameraPosition { get { return iris_load_vec4(IRIS_OFF_iris_CameraPosition); } } // vec4
    property float4 iris_ScreenSize { get { return iris_load_vec4(IRIS_OFF_iris_ScreenSize); } } // vec4
    property float4 iris_Time { get { return iris_load_vec4(IRIS_OFF_iris_Time); } } // vec4
    property float4x4 iris_ModelViewMatrixInverse { get { return iris_load_mat4(IRIS_OFF_iris_ModelViewMatrixInverse); } } // mat4
    property float4x4 iris_ProjectionMatrixInverse { get { return iris_load_mat4(IRIS_OFF_iris_ProjectionMatrixInverse); } } // mat4
    property float4 iris_FogColor { get { return iris_load_vec4(IRIS_OFF_iris_FogColor); } } // vec4
    property float4 iris_FogParams { get { return iris_load_vec4(IRIS_OFF_iris_FogParams); } } // vec4
    property float3 cameraPosition { get { return iris_load_vec3(IRIS_OFF_cameraPosition); } } // vec3
    property float3 cameraPositionFract { get { return iris_load_vec3(IRIS_OFF_cameraPositionFract); } } // vec3
    property int3 cameraPositionInt { get { return iris_load_ivec3(IRIS_OFF_cameraPositionInt); } } // ivec3
    property float3 previousCameraPosition { get { return iris_load_vec3(IRIS_OFF_previousCameraPosition); } } // vec3
    property int2 eyeBrightnessSmooth { get { return iris_load_ivec2(IRIS_OFF_eyeBrightnessSmooth); } } // ivec2
    property float far { get { return iris_load_float(IRIS_OFF_far); } } // float
    property float near { get { return iris_load_float(IRIS_OFF_near); } } // float
    property float3 fogColor { get { return iris_load_vec3(IRIS_OFF_fogColor); } } // vec3
    property int frameCounter { get { return iris_load_int(IRIS_OFF_frameCounter); } } // int
    property float frameTimeCounter { get { return iris_load_float(IRIS_OFF_frameTimeCounter); } } // float
    property float4x4 gbufferModelView { get { return iris_load_mat4(IRIS_OFF_gbufferModelView); } } // mat4
    property float4x4 gbufferModelViewInverse { get { return iris_load_mat4(IRIS_OFF_gbufferModelViewInverse); } } // mat4
    property float4x4 gbufferProjection { get { return iris_load_mat4(IRIS_OFF_gbufferProjection); } } // mat4
    property float4x4 gbufferProjectionInverse { get { return iris_load_mat4(IRIS_OFF_gbufferProjectionInverse); } } // mat4
    property float4x4 gbufferPreviousModelView { get { return iris_load_mat4(IRIS_OFF_gbufferPreviousModelView); } } // mat4
    property float4x4 gbufferPreviousProjection { get { return iris_load_mat4(IRIS_OFF_gbufferPreviousProjection); } } // mat4
    property int hasSkylight { get { return iris_load_int(IRIS_OFF_hasSkylight); } } // bool
    property int isEyeInWater { get { return iris_load_int(IRIS_OFF_isEyeInWater); } } // int
    property float rainStrength { get { return iris_load_float(IRIS_OFF_rainStrength); } } // float
    property float3 shadowLightPosition { get { return iris_load_vec3(IRIS_OFF_shadowLightPosition); } } // vec3
    property float4x4 shadowModelView { get { return iris_load_mat4(IRIS_OFF_shadowModelView); } } // mat4
    property float4x4 shadowProjection { get { return iris_load_mat4(IRIS_OFF_shadowProjection); } } // mat4
    property float3 sunPosition { get { return iris_load_vec3(IRIS_OFF_sunPosition); } } // vec3
    property float torchShadowBlend { get { return iris_load_float(IRIS_OFF_torchShadowBlend); } } // float
    property float4x4 torchShadowModelViewPrevious { get { return iris_load_mat4(IRIS_OFF_torchShadowModelViewPrevious); } } // mat4
};

static IrisFrame frame;
