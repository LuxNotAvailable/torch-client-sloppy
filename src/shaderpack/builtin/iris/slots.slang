// The resources a native program can name, each through a slot: a
// specialization constant the engine sets to the resource's place in the
// pass's slot table (`vk::pack::bindless::bind_precompiled`). Only what a
// program uses stays in its SPIR-V.

[SpecializationConstant] const uint IRIS_SLOT_colortex0 = 0;
[SpecializationConstant] const uint IRIS_SLOT_colortex0_sampler = 0;
#define colortex0 iris_texture(IRIS_SLOT_colortex0)
#define colortex0_sampler iris_sampler(IRIS_SLOT_colortex0_sampler)
[SpecializationConstant] const uint IRIS_SLOT_colortex1 = 0;
[SpecializationConstant] const uint IRIS_SLOT_colortex1_sampler = 0;
#define colortex1 iris_texture(IRIS_SLOT_colortex1)
#define colortex1_sampler iris_sampler(IRIS_SLOT_colortex1_sampler)
[SpecializationConstant] const uint IRIS_SLOT_colortex2 = 0;
[SpecializationConstant] const uint IRIS_SLOT_colortex2_sampler = 0;
#define colortex2 iris_texture(IRIS_SLOT_colortex2)
#define colortex2_sampler iris_sampler(IRIS_SLOT_colortex2_sampler)
[SpecializationConstant] const uint IRIS_SLOT_colortex3 = 0;
[SpecializationConstant] const uint IRIS_SLOT_colortex3_sampler = 0;
#define colortex3 iris_texture(IRIS_SLOT_colortex3)
#define colortex3_sampler iris_sampler(IRIS_SLOT_colortex3_sampler)
[SpecializationConstant] const uint IRIS_SLOT_colortex4 = 0;
[SpecializationConstant] const uint IRIS_SLOT_colortex4_sampler = 0;
#define colortex4 iris_texture(IRIS_SLOT_colortex4)
#define colortex4_sampler iris_sampler(IRIS_SLOT_colortex4_sampler)
[SpecializationConstant] const uint IRIS_SLOT_colortex5 = 0;
[SpecializationConstant] const uint IRIS_SLOT_colortex5_sampler = 0;
#define colortex5 iris_texture(IRIS_SLOT_colortex5)
#define colortex5_sampler iris_sampler(IRIS_SLOT_colortex5_sampler)
[SpecializationConstant] const uint IRIS_SLOT_colortex6 = 0;
[SpecializationConstant] const uint IRIS_SLOT_colortex6_sampler = 0;
#define colortex6 iris_texture(IRIS_SLOT_colortex6)
#define colortex6_sampler iris_sampler(IRIS_SLOT_colortex6_sampler)
[SpecializationConstant] const uint IRIS_SLOT_colortex7 = 0;
[SpecializationConstant] const uint IRIS_SLOT_colortex7_sampler = 0;
#define colortex7 iris_texture(IRIS_SLOT_colortex7)
#define colortex7_sampler iris_sampler(IRIS_SLOT_colortex7_sampler)
[SpecializationConstant] const uint IRIS_SLOT_colortex8 = 0;
[SpecializationConstant] const uint IRIS_SLOT_colortex8_sampler = 0;
#define colortex8 iris_texture(IRIS_SLOT_colortex8)
#define colortex8_sampler iris_sampler(IRIS_SLOT_colortex8_sampler)
[SpecializationConstant] const uint IRIS_SLOT_colortex9 = 0;
[SpecializationConstant] const uint IRIS_SLOT_colortex9_sampler = 0;
#define colortex9 iris_texture(IRIS_SLOT_colortex9)
#define colortex9_sampler iris_sampler(IRIS_SLOT_colortex9_sampler)
[SpecializationConstant] const uint IRIS_SLOT_colortex10 = 0;
[SpecializationConstant] const uint IRIS_SLOT_colortex10_sampler = 0;
#define colortex10 iris_texture(IRIS_SLOT_colortex10)
#define colortex10_sampler iris_sampler(IRIS_SLOT_colortex10_sampler)
[SpecializationConstant] const uint IRIS_SLOT_colortex11 = 0;
[SpecializationConstant] const uint IRIS_SLOT_colortex11_sampler = 0;
#define colortex11 iris_texture(IRIS_SLOT_colortex11)
#define colortex11_sampler iris_sampler(IRIS_SLOT_colortex11_sampler)
[SpecializationConstant] const uint IRIS_SLOT_colortex12 = 0;
[SpecializationConstant] const uint IRIS_SLOT_colortex12_sampler = 0;
#define colortex12 iris_texture(IRIS_SLOT_colortex12)
#define colortex12_sampler iris_sampler(IRIS_SLOT_colortex12_sampler)
[SpecializationConstant] const uint IRIS_SLOT_colortex13 = 0;
[SpecializationConstant] const uint IRIS_SLOT_colortex13_sampler = 0;
#define colortex13 iris_texture(IRIS_SLOT_colortex13)
#define colortex13_sampler iris_sampler(IRIS_SLOT_colortex13_sampler)
[SpecializationConstant] const uint IRIS_SLOT_colortex14 = 0;
[SpecializationConstant] const uint IRIS_SLOT_colortex14_sampler = 0;
#define colortex14 iris_texture(IRIS_SLOT_colortex14)
#define colortex14_sampler iris_sampler(IRIS_SLOT_colortex14_sampler)
[SpecializationConstant] const uint IRIS_SLOT_colortex15 = 0;
[SpecializationConstant] const uint IRIS_SLOT_colortex15_sampler = 0;
#define colortex15 iris_texture(IRIS_SLOT_colortex15)
#define colortex15_sampler iris_sampler(IRIS_SLOT_colortex15_sampler)
[SpecializationConstant] const uint IRIS_SLOT_depthtex0 = 0;
[SpecializationConstant] const uint IRIS_SLOT_depthtex0_sampler = 0;
#define depthtex0 iris_texture(IRIS_SLOT_depthtex0)
#define depthtex0_sampler iris_sampler(IRIS_SLOT_depthtex0_sampler)
[SpecializationConstant] const uint IRIS_SLOT_depthtex1 = 0;
[SpecializationConstant] const uint IRIS_SLOT_depthtex1_sampler = 0;
#define depthtex1 iris_texture(IRIS_SLOT_depthtex1)
#define depthtex1_sampler iris_sampler(IRIS_SLOT_depthtex1_sampler)
[SpecializationConstant] const uint IRIS_SLOT_depthtex2 = 0;
[SpecializationConstant] const uint IRIS_SLOT_depthtex2_sampler = 0;
#define depthtex2 iris_texture(IRIS_SLOT_depthtex2)
#define depthtex2_sampler iris_sampler(IRIS_SLOT_depthtex2_sampler)
[SpecializationConstant] const uint IRIS_SLOT_shadowtex0 = 0;
[SpecializationConstant] const uint IRIS_SLOT_shadowtex0_sampler = 0;
#define shadowtex0 iris_texture(IRIS_SLOT_shadowtex0)
#define shadowtex0_sampler iris_sampler(IRIS_SLOT_shadowtex0_sampler)
[SpecializationConstant] const uint IRIS_SLOT_shadowtex1 = 0;
[SpecializationConstant] const uint IRIS_SLOT_shadowtex1_sampler = 0;
#define shadowtex1 iris_texture(IRIS_SLOT_shadowtex1)
#define shadowtex1_sampler iris_sampler(IRIS_SLOT_shadowtex1_sampler)
[SpecializationConstant] const uint IRIS_SLOT_shadowcolor0 = 0;
[SpecializationConstant] const uint IRIS_SLOT_shadowcolor0_sampler = 0;
#define shadowcolor0 iris_texture(IRIS_SLOT_shadowcolor0)
#define shadowcolor0_sampler iris_sampler(IRIS_SLOT_shadowcolor0_sampler)
[SpecializationConstant] const uint IRIS_SLOT_shadowcolor1 = 0;
[SpecializationConstant] const uint IRIS_SLOT_shadowcolor1_sampler = 0;
#define shadowcolor1 iris_texture(IRIS_SLOT_shadowcolor1)
#define shadowcolor1_sampler iris_sampler(IRIS_SLOT_shadowcolor1_sampler)
[SpecializationConstant] const uint IRIS_SLOT_shadowcolor2 = 0;
[SpecializationConstant] const uint IRIS_SLOT_shadowcolor2_sampler = 0;
#define shadowcolor2 iris_texture(IRIS_SLOT_shadowcolor2)
#define shadowcolor2_sampler iris_sampler(IRIS_SLOT_shadowcolor2_sampler)
[SpecializationConstant] const uint IRIS_SLOT_shadowcolor3 = 0;
[SpecializationConstant] const uint IRIS_SLOT_shadowcolor3_sampler = 0;
#define shadowcolor3 iris_texture(IRIS_SLOT_shadowcolor3)
#define shadowcolor3_sampler iris_sampler(IRIS_SLOT_shadowcolor3_sampler)
[SpecializationConstant] const uint IRIS_SLOT_shadowcolor4 = 0;
[SpecializationConstant] const uint IRIS_SLOT_shadowcolor4_sampler = 0;
#define shadowcolor4 iris_texture(IRIS_SLOT_shadowcolor4)
#define shadowcolor4_sampler iris_sampler(IRIS_SLOT_shadowcolor4_sampler)
[SpecializationConstant] const uint IRIS_SLOT_shadowcolor5 = 0;
[SpecializationConstant] const uint IRIS_SLOT_shadowcolor5_sampler = 0;
#define shadowcolor5 iris_texture(IRIS_SLOT_shadowcolor5)
#define shadowcolor5_sampler iris_sampler(IRIS_SLOT_shadowcolor5_sampler)
[SpecializationConstant] const uint IRIS_SLOT_shadowcolor6 = 0;
[SpecializationConstant] const uint IRIS_SLOT_shadowcolor6_sampler = 0;
#define shadowcolor6 iris_texture(IRIS_SLOT_shadowcolor6)
#define shadowcolor6_sampler iris_sampler(IRIS_SLOT_shadowcolor6_sampler)
[SpecializationConstant] const uint IRIS_SLOT_shadowcolor7 = 0;
[SpecializationConstant] const uint IRIS_SLOT_shadowcolor7_sampler = 0;
#define shadowcolor7 iris_texture(IRIS_SLOT_shadowcolor7)
#define shadowcolor7_sampler iris_sampler(IRIS_SLOT_shadowcolor7_sampler)
[SpecializationConstant] const uint IRIS_SLOT_noisetex = 0;
[SpecializationConstant] const uint IRIS_SLOT_noisetex_sampler = 0;
#define noisetex iris_texture(IRIS_SLOT_noisetex)
#define noisetex_sampler iris_sampler(IRIS_SLOT_noisetex_sampler)
[SpecializationConstant] const uint IRIS_SLOT_gtexture = 0;
[SpecializationConstant] const uint IRIS_SLOT_gtexture_sampler = 0;
#define gtexture iris_texture(IRIS_SLOT_gtexture)
#define gtexture_sampler iris_sampler(IRIS_SLOT_gtexture_sampler)
[SpecializationConstant] const uint IRIS_SLOT_lightmap = 0;
[SpecializationConstant] const uint IRIS_SLOT_lightmap_sampler = 0;
#define lightmap iris_texture(IRIS_SLOT_lightmap)
#define lightmap_sampler iris_sampler(IRIS_SLOT_lightmap_sampler)
[SpecializationConstant] const uint IRIS_SLOT_normals = 0;
[SpecializationConstant] const uint IRIS_SLOT_normals_sampler = 0;
#define normals iris_texture(IRIS_SLOT_normals)
#define normals_sampler iris_sampler(IRIS_SLOT_normals_sampler)
[SpecializationConstant] const uint IRIS_SLOT_specular = 0;
[SpecializationConstant] const uint IRIS_SLOT_specular_sampler = 0;
#define specular iris_texture(IRIS_SLOT_specular)
#define specular_sampler iris_sampler(IRIS_SLOT_specular_sampler)

// Hardware depth comparison: a depth view and a comparison sampler.
[SpecializationConstant] const uint IRIS_SLOT_shadowtex0HW = 0;
[SpecializationConstant] const uint IRIS_SLOT_shadowtex0HW_sampler = 0;
#define shadowtex0HW iris_depth_texture(IRIS_SLOT_shadowtex0HW)
#define shadowtex0HW_sampler iris_comparison_sampler(IRIS_SLOT_shadowtex0HW_sampler)
[SpecializationConstant] const uint IRIS_SLOT_shadowtex1HW = 0;
[SpecializationConstant] const uint IRIS_SLOT_shadowtex1HW_sampler = 0;
#define shadowtex1HW iris_depth_texture(IRIS_SLOT_shadowtex1HW)
#define shadowtex1HW_sampler iris_comparison_sampler(IRIS_SLOT_shadowtex1HW_sampler)

// Buffer objects: `IRIS_BUFFER_<n>` on a line declares bufferObject.<n>,
// read-write when the line says read_write (`compile::native::declare`).
[SpecializationConstant] const uint IRIS_SLOT_iris_buffer_0 = 0;
#define IRIS_BUFFER_0 IRIS_SLOT_iris_buffer_0
[SpecializationConstant] const uint IRIS_SLOT_iris_buffer_1 = 0;
#define IRIS_BUFFER_1 IRIS_SLOT_iris_buffer_1
[SpecializationConstant] const uint IRIS_SLOT_iris_buffer_2 = 0;
#define IRIS_BUFFER_2 IRIS_SLOT_iris_buffer_2
[SpecializationConstant] const uint IRIS_SLOT_iris_buffer_3 = 0;
#define IRIS_BUFFER_3 IRIS_SLOT_iris_buffer_3
[SpecializationConstant] const uint IRIS_SLOT_iris_buffer_4 = 0;
#define IRIS_BUFFER_4 IRIS_SLOT_iris_buffer_4
[SpecializationConstant] const uint IRIS_SLOT_iris_buffer_5 = 0;
#define IRIS_BUFFER_5 IRIS_SLOT_iris_buffer_5
[SpecializationConstant] const uint IRIS_SLOT_iris_buffer_6 = 0;
#define IRIS_BUFFER_6 IRIS_SLOT_iris_buffer_6
[SpecializationConstant] const uint IRIS_SLOT_iris_buffer_7 = 0;
#define IRIS_BUFFER_7 IRIS_SLOT_iris_buffer_7
