# Java renderer code to the mod API

How the Java a client renderer uses maps onto `torch-mod-api`
(`docs/MOD_API.md`). Vanilla first, then common mod libraries. Each row
says what to look for and what to write.

## The renderer itself

| Java | Port |
|---|---|
| `class X implements BlockEntityRenderer<T>` | a branch of your `Plugin::block_entity`, matched on `view.kind` |
| `BlockEntityRenderers.register(TYPE, X::new)` / Registrate `.renderer(() -> X::new)` | the type's id in `Registration.block_entities` |
| `render(T be, float partialTicks, PoseStack ms, MultiBufferSource buf, int light, int overlay)` | `block_entity(&mut self, view: &BlockEntityView) -> Rig`, called on change, not per frame |
| `partialTicks`, `level.getGameTime()`, `AnimationTickHolder.getRenderTime()` | the time an `Animation` is played at: don't compute angles from them, describe the motion |
| `shouldRenderOffScreen`, `getViewDistance` | nothing: the client decides culling |

## Reading state

| Java | Port |
|---|---|
| `be.getBlockState().getValue(AXIS)` | `view.prop("axis")` |
| `state.getBlock()`, `BlockState.is(...)` | `view.block` (the id) |
| a field set from NBT in `read(tag, clientPacket)` / `loadAdditional` | `view.nbt` by the same key (`view.nbt_f64(&["Speed"])`) |
| `be.getLevel().getBlockState(pos.relative(dir))` | `host::block(x, y, z)` (v1 sees 2 blocks around) |
| `Direction`, `Direction.Axis` | your own small enums; `axis_vector` in `ports/example` |

## Placing parts

`PoseStack` calls before a draw become the part's `Transform` (static) or
`Animation` (changes with time). Java works in blocks (1.0 = a block);
the API works in sixteenths, so multiply translations by 16.

| Java | Port |
|---|---|
| `ms.translate(x, y, z)` | `transform.translate = [x*16, y*16, z*16]` |
| `ms.mulPose(Axis.YP.rotationDegrees(a))` about the block centre (`translate(.5,.5,.5)` ... `translate(-.5,-.5,-.5)`) | `transform.rotate = [0, a, 0]`, `pivot = [8, 8, 8]` |
| a rotation about another point | the same with that `pivot` (in sixteenths) |
| `ms.scale(s, s, s)` | `transform.scale = [s, s, s]` (about `pivot`) |
| a rotation whose angle is `time * speed` | `Animation::Spin { axis, pivot, degrees_per_tick: speed, phase }` |
| `Mth.sin(time * k) * amp` used as an angle | `Animation::Swing { amplitude: amp, period_ticks: 2pi/k, ... }` |
| an offset that moves back and forth | `Animation::Slide` |

Several `mulPose` calls compose: the API applies X, then Y, then Z. Java
applies the last call to the vertex first, so `rotateY(a).rotateX(b)` is
`[b, a, 0]`; any other order (Z before Y) needs converting: the Create
port's `dir::java_rotations` takes the calls in Java order and returns the
API's angles. For a longer chain of `translate`/`rotate`/`scale` calls
(an item's pose on a belt), replay them on the Create port's `pose::Pose`
and take `.transform()`: one `Transform` with a pivot of 0.

Time in the NBT: something the server sends once and the client then
advances (an angle, an item's position) is as of `view.nbt_time`, not
`view.time`: a view is made again with old NBT when a neighbour changes.

## Drawing

| Java | Port |
|---|---|
| a model drawn from `ModelManager` / a baked model by id | `Geometry::Model { model: "<ns>:block/<path>" }` |
| `blockRenderer.renderSingleBlock(state, ...)` | `Geometry::Block { block, props }` |
| a hand-built box (`VertexConsumer` vertices) with one sprite | `Geometry::Cuboid { from, to, texture, faces, tint, uv }` |
| a fluid box (`FluidRenderer.renderFluidBox`, a fluid's still sprite and tint) | `Geometry::Cuboid` with the fluid's still sprite (`minecraft:block/water_still`) and its tint |
| `itemRenderer.render(stack, ItemDisplayContext.FIXED, ...)` / `renderStatic` | `Part::item(id)` with the pose up to the call; the `isGui3d() ? a : b` branch as `transform` (3D) and `Geometry::Item.flat` (sprite) |
| something moved each client tick (`tick()` advancing a position) | `Animation::Path` through where it will be, from `view.nbt_time`; the server's next update replaces it |
| `RenderType.translucent()` and other layers | not in v1: parts draw cutout |
| packed `light` | nothing: the client lights parts at the block entity |

## Create and the Flywheel / Catnip libraries

Many mods build on Create's rendering helpers. These are just as general;
the same mapping applies to any mod using them.

| Java | Port |
|---|---|
| `PartialModel.of(Create.asResource("block/shaft_half"))` | `Geometry::Model { model: "create:block/shaft_half" }` |
| `CachedBuffers.partial(PARTIAL, state)` / `CachedBufferer.partialFacing(...)` | a `Part` with that model, turned by `x`/`y` quarter turns for facing |
| `SuperByteBuffer.rotateCentered(angle, axis)` | `transform.rotate` (static) or `Animation::Spin` (from speed) about `[8, 8, 8]` |
| `KineticBlockEntityRenderer.getAngleForBe(be, pos, axis)`: angle from `AnimationTickHolder.getRenderTime() * speed * 3 / 10` plus an offset | `Animation::Spin { degrees_per_tick: speed * 0.3, phase: <the offset> }`; `speed` is the block entity's `Speed` NBT (rpm) |
| `getRotationOffsetForPosition(be, pos, axis)` (cogs meshing by position parity) | `phase`: compute it from `view.pos` the same way |
| `SuperByteBuffer.shiftUVScrolling(SpriteShiftEntry, scroll)` with `scroll = fract(t) * targetHeight * k` | `Part::with_scroll(Scroll { from, to, per_tick, phase, span: k })`; declare `to` in `sprites` |
| Flywheel `Visual`s / instancing (`createInstance`, `beginFrame`) | the same as the renderer they replace: parts and animations |

## When nothing fits

Write it in `PORTED.md` under "Needs from the API", with the Java class
and what it draws. Don't fake it.
