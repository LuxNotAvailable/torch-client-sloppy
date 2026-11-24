# Create port

- Mod: create 6.0.10 (`create-1.21.1-6.0.10.jar`, sha256 ef87fe57...e37a)
- Licence: code MIT (this port may be shared); assets All Rights Reserved (the `.torchmod` itself, which carries them, may not)
- API: 1
- Source: decompiled with Vineflower 1.12.0; Catnip's `CachedBuffers` and `AngleHelper` from the bundled Ponder jar

## Renderers

| Block entity type | Java renderer | Status | Notes |
|---|---|---|---|
| `create:schematicannon` | `SchematicannonRenderer` | not yet |  |
| `create:simple_kinetic` | `BracketedKineticBlockEntityRenderer` | done | own model turning; a large cog's wheel and shaft apart |
| `create:motor` | `CreativeMotorRenderer` | done | half shaft |
| `create:gearbox` | `GearboxRenderer` | done | half shafts, turned by the drive's side |
| `create:encased_shaft` | `ShaftRenderer` | done | a shaft in the casing |
| `create:encased_cogwheel` | `EncasedCogRenderer` | done | cog and the half shafts it has |
| `create:encased_large_cogwheel` | `EncasedCogRenderer` | done | cog and the half shafts it has |
| `create:adjustable_chain_gearshift` | `ShaftRenderer` | done | a shaft in the casing |
| `create:encased_fan` | `EncasedFanRenderer` | done | shaft and propeller out the back; the propeller at 5× the speed, clamped; the air current (particles) isn't drawn |
| `create:clutch` | `SplitShaftRenderer` | done | half shafts; far side stops when powered |
| `create:gearshift` | `SplitShaftRenderer` | done | half shafts; far side reverses when powered |
| `create:turntable` | `KineticBlockEntityRenderer` | done | own model turning |
| `create:hand_crank` | `HandCrankRenderer` | done | own model turning; the handle at the speed its angle chases |
| `create:valve_handle` | `HandCrankRenderer` | next | round 2 |
| `create:cuckoo_clock` | `CuckooClockRenderer` | not yet |  |
| `create:gantry_shaft` | `KineticBlockEntityRenderer` | done | own model turning |
| `create:gantry_pinion` | `GantryCarriageRenderer` | not yet |  |
| `create:chain_conveyor` | `ChainConveyorRenderer` | not yet |  |
| `create:mechanical_pump` | `PumpRenderer` | done | the cog; drains (`PipeAttachmentModel`) |
| `create:smart_fluid_pipe` | `SmartBlockEntityRenderer` | partial | rims (`PipeAttachmentModel`); the filter item not yet |
| `create:glass_fluid_pipe` | `TransparentStraightPipeRenderer` | partial | rims (`PipeAttachmentModel`); the fluid inside isn't drawn |
| `create:fluid_valve` | `FluidValveRenderer` | partial | rims (`PipeAttachmentModel`); the pointer and shaft aren't drawn |
| `create:fluid_tank` | `FluidTankRenderer` | done | fluid box; boiler gauges skipped; one sprite stretched over the box |
| `create:creative_fluid_tank` | `FluidTankRenderer` | done | fluid box |
| `create:hose_pulley` | `HosePulleyRenderer` | not yet |  |
| `create:spout` | `SpoutRenderer` | not yet |  |
| `create:item_drain` | `ItemDrainRenderer` | not yet |  |
| `create:belt` | `BeltRenderer` | partial | segments with scrolling texture, dyes and pulleys; items glide to the end and queue (slope tilt as of the update; upright items, packages, shadows skipped; lit at the controller) |
| `create:chute` | `ChuteRenderer` | not yet | items: `Part::item` |
| `create:smart_chute` | `SmartChuteRenderer` | not yet |  |
| `create:andesite_tunnel` | `BeltTunnelRenderer` | not yet |  |
| `create:brass_tunnel` | `BeltTunnelRenderer` | not yet |  |
| `create:mechanical_arm` | `ArmRenderer` | next | round 2 |
| `create:item_hatch` | `SmartBlockEntityRenderer` | not yet |  |
| `create:packager` | `PackagerRenderer` | not yet |  |
| `create:repackager` | `PackagerRenderer` | not yet |  |
| `create:package_frogport` | `FrogportRenderer` | not yet |  |
| `create:package_postbox` | `PostboxRenderer` | not yet |  |
| `create:table_cloth` | `TableClothRenderer` | not yet |  |
| `create:packager_link` | `LinkBulbRenderer` | not yet |  |
| `create:mechanical_piston` | `MechanicalPistonRenderer` | next | round 2 |
| `create:windmill_bearing` | `BearingRenderer` | partial | half shaft and plate; the sails (a contraption entity) aren't drawn |
| `create:mechanical_bearing` | `BearingRenderer` | partial | half shaft and plate; the contraption isn't drawn |
| `create:clockwork_bearing` | `BearingRenderer` | next | round 2 |
| `create:rope_pulley` | `PulleyRenderer` | not yet |  |
| `create:elevator_pulley` | `ElevatorPulleyRenderer` | not yet |  |
| `create:sticker` | `StickerRenderer` | not yet |  |
| `create:contraption_controls` | `ContraptionControlsRenderer` | not yet |  |
| `create:drill` | `DrillRenderer` | not yet |  |
| `create:saw` | `SawRenderer` | not yet |  |
| `create:harvester` | `HarvesterRenderer` | not yet |  |
| `create:mechanical_roller` | `RollerRenderer` | not yet |  |
| `create:portable_storage_interface` | `PortableStorageInterfaceRenderer` | not yet |  |
| `create:portable_fluid_interface` | `PortableStorageInterfaceRenderer` | not yet |  |
| `create:steam_engine` | `SteamEngineRenderer` | next | round 2 |
| `create:steam_whistle` | `WhistleRenderer` | not yet |  |
| `create:powered_shaft` | `KineticBlockEntityRenderer` | done | own model turning, +180 on even coordinates |
| `create:flywheel` | `FlywheelRenderer` | next | round 2 |
| `create:millstone` | `MillstoneRenderer` | next | round 2 |
| `create:crushing_wheel` | `KineticBlockEntityRenderer` | done | own model turning |
| `create:water_wheel` | `WaterWheelRenderer` | done | wheel turning; wood swap (`Material`) isn't ported: oak |
| `create:large_water_wheel` | `WaterWheelRenderer` | done | wheel turning; wood swap isn't ported: oak |
| `create:mechanical_press` | `MechanicalPressRenderer` | next | round 2 |
| `create:mechanical_mixer` | `MechanicalMixerRenderer` | next | round 2 |
| `create:deployer` | `DeployerRenderer` | next | round 2 |
| `create:basin` | `BasinRenderer` | not yet | items: `Part::item` |
| `create:blaze_heater` | `BlazeBurnerRenderer` | not yet |  |
| `create:mechanical_crafter` | `MechanicalCrafterRenderer` | not yet |  |
| `create:sequenced_gearshift` | `SplitShaftRenderer` | done | half shafts; its instruction's modifier isn't ported |
| `create:rotation_speed_controller` | `SpeedControllerRenderer` | partial | the shaft; the bracket needs a client-side check of the cog above |
| `create:speedometer` | `GaugeRenderer` | next | round 2: needle as a swing |
| `create:stressometer` | `GaugeRenderer` | next | round 2: needle |
| `create:analog_lever` | `AnalogLeverRenderer` | not yet |  |
| `create:placard` | `PlacardRenderer` | not yet | items: `Part::item` |
| `create:factory_panel` | `FactoryPanelRenderer` | not yet | needs text and items |
| `create:redstone_link` | `SmartBlockEntityRenderer` | not yet |  |
| `create:nixie_tube` | `NixieTubeRenderer` | not yet | needs text rendering |
| `create:display_link` | `LinkBulbRenderer` | not yet |  |
| `create:stockpile_switch` | `SmartBlockEntityRenderer` | not yet |  |
| `create:creative_crate` | `SmartBlockEntityRenderer` | not yet |  |
| `create:depot` | `DepotRenderer` | done | held stack and outputs; an incoming item's ease to the centre isn't played |
| `create:weighted_ejector` | `EjectorRenderer` | not yet |  |
| `create:funnel` | `FunnelRenderer` | not yet | items: `Part::item` |
| `create:content_observer` | `SmartBlockEntityRenderer` | not yet |  |
| `create:pulse_extender` | `BrassDiodeRenderer` | not yet |  |
| `create:pulse_repeater` | `BrassDiodeRenderer` | not yet |  |
| `create:pulse_timer` | `BrassDiodeRenderer` | not yet |  |
| `create:lectern_controller` | `LecternControllerRenderer` | not yet |  |
| `create:backtank` | `BacktankRenderer` | not yet |  |
| `create:peculiar_bell` | `BellRenderer` | not yet |  |
| `create:cursed_bell` | `BellRenderer` | not yet |  |
| `create:desk_bell` | `DeskBellRenderer` | not yet |  |
| `create:toolbox` | `ToolboxRenderer` | not yet |  |
| `create:track` | `TrackRenderer` | not yet | needs curved track geometry |
| `create:bogey` | `BogeyBlockEntityRenderer` | not yet | needs entity-side trains |
| `create:track_station` | `StationRenderer` | not yet | needs text |
| `create:sliding_door` | `SlidingDoorRenderer` | not yet |  |
| `create:flap_display` | `FlapDisplayRenderer` | not yet | needs text |
| `create:track_signal` | `SignalRenderer` | not yet |  |
| `create:track_observer` | `TrackObserverRenderer` | not yet |  |

## Baked-model wrappers

| Block | Java model | Status | Notes |
|---|---|---|---|
| `create:fluid_pipe` | `PipeAttachmentModel` | done | arms, rims, drains, junction casing, bracket; as rig parts (no renderer in Java) |
| `create:encased_fluid_pipe` | `PipeAttachmentModel` | done | drains only, as in Java |

Gaps: drains only face Create's own fluid holders (another mod's tank gets a
rim), and a neighbour pipe's bracket is taken as absent.

## Needs from the API

- Texture overrides on a part (`WaterWheelRenderer` swaps the oak templates for the wheel's `Material` wood).
- Text rendering (nixie tubes, flap displays, stations, factory panels).
- Tiled UVs on a cuboid (a tall tank's fluid shows one sprite stretched; `renderFluidBox` tiles it per block).
- Which block entities have a fluid capability (`FluidHandler.BLOCK`): a converter fact would let pipes drain into any mod's tank.
- Server config values (`fluidTankCapacity`: levels assume the default 8 buckets a block).
