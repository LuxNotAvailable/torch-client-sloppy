package torchdump;

import java.io.IOException;
import java.io.Writer;
import java.nio.charset.StandardCharsets;
import java.nio.file.Files;
import java.nio.file.Path;
import java.util.ArrayList;
import java.util.HashMap;
import java.util.List;
import java.util.Map;
import java.util.function.Supplier;

import com.google.gson.Gson;
import com.google.gson.GsonBuilder;
import com.google.gson.JsonArray;
import com.google.gson.JsonElement;
import com.google.gson.JsonNull;
import com.google.gson.JsonObject;
import com.google.gson.JsonPrimitive;

import net.minecraft.core.BlockPos;
import net.minecraft.core.registries.BuiltInRegistries;
import net.minecraft.world.entity.EntityType;
import net.minecraft.world.item.BlockItem;
import net.minecraft.world.item.Item;
import net.minecraft.world.item.ItemStack;
import net.minecraft.world.level.EmptyBlockGetter;
import net.minecraft.world.level.block.Block;
import net.minecraft.world.level.block.SoundType;
import net.minecraft.world.level.block.entity.BlockEntityType;
import net.minecraft.world.level.block.state.BlockState;
import net.minecraft.world.level.block.state.properties.Property;
import net.minecraft.world.level.material.FluidState;
import net.minecraft.world.phys.AABB;
import net.minecraft.world.phys.shapes.VoxelShape;
import net.neoforged.bus.api.IEventBus;
import net.neoforged.fml.ModList;
import net.neoforged.fml.common.Mod;
import net.neoforged.neoforge.common.NeoForge;
import net.neoforged.neoforge.event.server.ServerAboutToStartEvent;

/**
 * Writes what Torch needs from a modpack that only running it can tell:
 * every block with its properties and states in network order, each
 * state's shapes and behaviour, block entity types, entity sizes and item
 * basics. Runs once the registries are frozen, then stops the server
 * before a world loads. See docs/MOD_TRANSLATOR_PLAN.md, step 2.
 *
 * Output: the `torchdump.out` system property, else torch-dump.json.
 */
@Mod("torchdump")
public class TorchDump {
    static final int FORMAT = 1;

    public TorchDump(IEventBus modBus) {
        NeoForge.EVENT_BUS.addListener(TorchDump::onAboutToStart);
    }

    static void onAboutToStart(ServerAboutToStartEvent event) {
        Path out = Path.of(System.getProperty("torchdump.out", "torch-dump.json"));
        int status = 0;
        try {
            write(out);
            System.out.println("[torchdump] wrote " + out.toAbsolutePath());
        } catch (Throwable e) {
            e.printStackTrace();
            status = 1;
        }
        // No world: the dump is all this server was started for.
        Runtime.getRuntime().halt(status);
    }

    static void write(Path out) throws IOException {
        JsonObject root = new JsonObject();
        root.addProperty("format", FORMAT);
        root.add("mods", mods());
        Shapes shapes = new Shapes();
        Facts facts = new Facts();
        root.add("blocks", blocks(shapes, facts));
        root.add("facts", facts.list);
        root.add("shapes", shapes.list);
        root.add("block_entities", blockEntities());
        root.add("entities", entities());
        root.add("items", items());
        root.add("connected_textures", connectedTextures());
        Gson gson = new GsonBuilder().disableHtmlEscaping().create();
        try (Writer w = Files.newBufferedWriter(out, StandardCharsets.UTF_8)) {
            gson.toJson(root, w);
        }
    }

    static JsonArray mods() {
        JsonArray out = new JsonArray();
        for (var info : ModList.get().getMods()) {
            JsonObject mod = new JsonObject();
            mod.addProperty("id", info.getModId());
            mod.addProperty("version", info.getVersion().toString());
            out.add(mod);
        }
        return out;
    }

    /** Distinct shapes as box lists; 0 is empty, 1 the full cube. */
    static class Shapes {
        final JsonArray list = new JsonArray();
        final Map<String, Integer> seen = new HashMap<>();

        Shapes() {
            index(new JsonArray());
            JsonArray cube = new JsonArray();
            cube.add(box(new AABB(0, 0, 0, 1, 1, 1)));
            index(cube);
        }

        JsonElement of(Supplier<VoxelShape> shape) {
            try {
                JsonArray boxes = new JsonArray();
                for (AABB b : shape.get().toAabbs()) {
                    boxes.add(box(b));
                }
                return new JsonPrimitive(index(boxes));
            } catch (Throwable e) {
                // A mod's shape that needs a real level.
                return JsonNull.INSTANCE;
            }
        }

        int index(JsonArray boxes) {
            return seen.computeIfAbsent(boxes.toString(), k -> {
                list.add(boxes);
                return list.size() - 1;
            });
        }

        static JsonArray box(AABB b) {
            JsonArray a = new JsonArray();
            for (double v : new double[] {b.minX, b.minY, b.minZ, b.maxX, b.maxY, b.maxZ}) {
                a.add(Math.round(v * 1e6) / 1e6);
            }
            return a;
        }
    }

    /** Distinct per-state behaviour rows. */
    static class Facts {
        final JsonArray list = new JsonArray();
        final Map<String, Integer> seen = new HashMap<>();

        int index(JsonObject fact) {
            return seen.computeIfAbsent(fact.toString(), k -> {
                list.add(fact);
                return list.size() - 1;
            });
        }
    }

    static JsonArray blocks(Shapes shapes, Facts facts) {
        var level = EmptyBlockGetter.INSTANCE;
        var pos = BlockPos.ZERO;
        JsonArray out = new JsonArray();
        for (Block block : BuiltInRegistries.BLOCK) {
            JsonObject b = new JsonObject();
            b.addProperty("name", BuiltInRegistries.BLOCK.getKey(block).toString());
            List<Property<?>> props = new ArrayList<>(block.getStateDefinition().getProperties());
            JsonArray properties = new JsonArray();
            List<List<Object>> values = new ArrayList<>();
            for (Property<?> p : props) {
                JsonArray names = new JsonArray();
                List<Object> order = new ArrayList<>(p.getPossibleValues());
                for (Object v : order) {
                    names.add(valueName(p, v));
                }
                JsonArray entry = new JsonArray();
                entry.add(p.getName());
                entry.add(names);
                properties.add(entry);
                values.add(order);
            }
            b.add("properties", properties);
            b.addProperty("friction", block.getFriction());
            b.addProperty("speed_factor", block.getSpeedFactor());
            b.addProperty("jump_factor", block.getJumpFactor());
            b.addProperty("explosion_resistance", block.getExplosionResistance());
            b.add("sound", sound(block.defaultBlockState().getSoundType()));
            b.addProperty("default", block.getStateDefinition().getPossibleStates().indexOf(block.defaultBlockState()));
            JsonArray states = new JsonArray();
            for (BlockState state : block.getStateDefinition().getPossibleStates()) {
                JsonObject f = new JsonObject();
                f.add("collision", shapes.of(() -> state.getCollisionShape(level, pos)));
                f.add("outline", shapes.of(() -> state.getShape(level, pos)));
                f.addProperty("light", state.getLightEmission());
                f.add("light_block", attempt(() -> new JsonPrimitive(state.getLightBlock(level, pos))));
                f.addProperty("occludes", state.canOcclude());
                f.add("solid_render", attempt(() -> new JsonPrimitive(state.isSolidRender(level, pos))));
                f.addProperty("render", state.getRenderShape().name());
                f.add("destroy_time", attempt(() -> new JsonPrimitive(state.getDestroySpeed(level, pos))));
                f.addProperty("needs_tool", state.requiresCorrectToolForDrops());
                f.addProperty("air", state.isAir());
                f.addProperty("replaceable", state.canBeReplaced());
                f.addProperty("block_entity", state.hasBlockEntity());
                f.addProperty("push", state.getPistonPushReaction().name());
                f.addProperty("random_ticks", state.isRandomlyTicking());
                f.addProperty("offset", state.hasOffsetFunction());
                f.add("fluid", fluid(state.getFluidState()));
                JsonArray row = new JsonArray();
                row.add(facts.index(f));
                for (int i = 0; i < props.size(); i++) {
                    row.add(values.get(i).indexOf(state.getValue(props.get(i))));
                }
                states.add(row);
            }
            b.add("states", states);
            out.add(b);
        }
        return out;
    }

    @SuppressWarnings({"unchecked", "rawtypes"})
    static String valueName(Property p, Object v) {
        return p.getName((Comparable) v);
    }

    static JsonElement attempt(Supplier<JsonElement> value) {
        try {
            return value.get();
        } catch (Throwable e) {
            return JsonNull.INSTANCE;
        }
    }

    static JsonElement fluid(FluidState fluid) {
        if (fluid.isEmpty()) {
            return JsonNull.INSTANCE;
        }
        JsonObject f = new JsonObject();
        f.addProperty("type", BuiltInRegistries.FLUID.getKey(fluid.getType()).toString());
        f.addProperty("amount", fluid.getAmount());
        f.addProperty("source", fluid.isSource());
        return f;
    }

    static JsonObject sound(SoundType sound) {
        JsonObject s = new JsonObject();
        s.addProperty("volume", sound.getVolume());
        s.addProperty("pitch", sound.getPitch());
        s.addProperty("break", sound.getBreakSound().getLocation().toString());
        s.addProperty("step", sound.getStepSound().getLocation().toString());
        s.addProperty("place", sound.getPlaceSound().getLocation().toString());
        s.addProperty("hit", sound.getHitSound().getLocation().toString());
        s.addProperty("fall", sound.getFallSound().getLocation().toString());
        return s;
    }

    static JsonArray blockEntities() {
        JsonArray out = new JsonArray();
        for (BlockEntityType<?> type : BuiltInRegistries.BLOCK_ENTITY_TYPE) {
            JsonObject t = new JsonObject();
            t.addProperty("name", BuiltInRegistries.BLOCK_ENTITY_TYPE.getKey(type).toString());
            JsonArray blocks = new JsonArray();
            for (Block block : type.getValidBlocks()) {
                blocks.add(BuiltInRegistries.BLOCK.getKey(block).toString());
            }
            t.add("blocks", blocks);
            out.add(t);
        }
        return out;
    }

    static JsonArray entities() {
        JsonArray out = new JsonArray();
        for (EntityType<?> type : BuiltInRegistries.ENTITY_TYPE) {
            JsonObject e = new JsonObject();
            e.addProperty("name", BuiltInRegistries.ENTITY_TYPE.getKey(type).toString());
            e.addProperty("width", type.getWidth());
            e.addProperty("height", type.getHeight());
            e.addProperty("eye_height", type.getDimensions().eyeHeight());
            e.addProperty("fixed", type.getDimensions().fixed());
            e.addProperty("category", type.getCategory().getName());
            e.addProperty("fire_immune", type.fireImmune());
            e.addProperty("tracking_range", type.clientTrackingRange());
            e.addProperty("update_interval", type.updateInterval());
            out.add(e);
        }
        return out;
    }

    /**
     * Create's connected textures (and its addons'): every sprite shift
     * `CTSpriteShifter` has made. They're made in each mod's sprite-shift
     * holder class, which is loaded here; on a server the cache still keys
     * each one as `<texture>-><sheet>+<type id>`. Read by reflection, so
     * no mod is needed to build this.
     */
    static JsonArray connectedTextures() {
        JsonArray out = new JsonArray();
        Class<?> shifter;
        try {
            shifter = Class.forName("com.simibubi.create.foundation.block.connected.CTSpriteShifter");
        } catch (Throwable e) {
            return out; // no Create
        }
        ClassLoader loader = shifter.getClassLoader();
        for (var scan : ModList.get().getAllScanData()) {
            for (var data : scan.getClasses()) {
                String name = data.clazz().getClassName();
                if (!name.contains("SpriteShift") || name.contains("$") || name.endsWith("CTSpriteShifter")) {
                    continue;
                }
                try {
                    Class.forName(name, true, loader);
                } catch (Throwable e) {
                    System.out.println("[torchdump] " + name + " didn't load: " + e);
                }
            }
        }
        try {
            java.lang.reflect.Field field = shifter.getDeclaredField("ENTRY_CACHE");
            field.setAccessible(true);
            Map<?, ?> cache = (Map<?, ?>) field.get(null);
            for (var entry : cache.entrySet()) {
                String key = String.valueOf(entry.getKey());
                int arrow = key.indexOf("->");
                int plus = key.lastIndexOf('+');
                if (arrow < 0 || plus < arrow) {
                    continue;
                }
                Object value = entry.getValue();
                Object type = value.getClass().getMethod("getType").invoke(value);
                int sheet = (int) type.getClass().getMethod("getSheetSize").invoke(type);
                JsonObject ct = new JsonObject();
                ct.addProperty("original", key.substring(0, arrow));
                ct.addProperty("connected", key.substring(arrow + 2, plus));
                ct.addProperty("type", key.substring(plus + 1));
                ct.addProperty("sheet", sheet);
                out.add(ct);
            }
        } catch (Throwable e) {
            System.out.println("[torchdump] connected textures unreadable: " + e);
        }
        return out;
    }

    static JsonArray items() {
        JsonArray out = new JsonArray();
        for (Item item : BuiltInRegistries.ITEM) {
            JsonObject i = new JsonObject();
            i.addProperty("name", BuiltInRegistries.ITEM.getKey(item).toString());
            ItemStack stack = new ItemStack(item);
            i.addProperty("max_stack", stack.getMaxStackSize());
            i.addProperty("max_damage", stack.getMaxDamage());
            if (item instanceof BlockItem blockItem) {
                i.addProperty("block", BuiltInRegistries.BLOCK.getKey(blockItem.getBlock()).toString());
            }
            out.add(i);
        }
        return out;
    }
}
