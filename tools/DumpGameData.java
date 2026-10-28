import java.util.ArrayList;
import java.util.List;

import net.minecraft.SharedConstants;
import net.minecraft.commands.Commands;
import net.minecraft.core.HolderLookup;
import net.minecraft.core.component.DataComponents;
import net.minecraft.core.registries.BuiltInRegistries;
import net.minecraft.network.chat.contents.TranslatableContents;
import net.minecraft.server.Bootstrap;
import net.minecraft.server.WorldLoader;
import net.minecraft.server.packs.repository.PackRepository;
import net.minecraft.server.packs.repository.ServerPacksSource;
import net.minecraft.server.permissions.LevelBasedPermissionSet;
import net.minecraft.util.Util;
import net.minecraft.world.flag.FeatureFlags;
import net.minecraft.world.item.CreativeModeTab;
import net.minecraft.world.item.CreativeModeTabs;
import net.minecraft.world.item.Item;
import net.minecraft.world.item.ItemStack;
import net.minecraft.world.item.Rarity;
import net.minecraft.world.level.DataPackConfig;
import net.minecraft.world.level.WorldDataConfiguration;
import net.minecraft.world.level.block.Block;
import net.minecraft.world.level.block.state.BlockState;

/**
 * Prints what the generators in this folder need from a running game, so they
 * do not have to parse decompiled source. One record per line:
 *
 *   block <id> <PushReaction> <hasBlockEntity>
 *   item  <id> <rarity> <maxStackSize> <maxDamage>
 *   tab   <id> <TOP|BOTTOM> <column> <title key> <icon id> <item id>...
 *
 * Run it with the server jar and its libraries on the classpath.
 */
public class DumpGameData {
    public static void main(String[] args) throws Exception {
        SharedConstants.tryDetectVersion();
        Bootstrap.bootStrap();
        HolderLookup.Provider registries = loadVanilla();
        StringBuilder out = new StringBuilder();

        for (Block block : BuiltInRegistries.BLOCK) {
            BlockState state = block.defaultBlockState();
            out.append("block ").append(BuiltInRegistries.BLOCK.getKey(block).getPath())
                .append(' ').append(state.getPistonPushReaction())
                .append(' ').append(state.hasBlockEntity()).append('\n');
        }

        for (Item item : BuiltInRegistries.ITEM) {
            ItemStack stack = item.getDefaultInstance();
            Rarity rarity = stack.getOrDefault(DataComponents.RARITY, Rarity.COMMON);
            out.append("item ").append(BuiltInRegistries.ITEM.getKey(item).getPath())
                .append(' ').append(rarity.getSerializedName())
                .append(' ').append(stack.getMaxStackSize())
                .append(' ').append(stack.getMaxDamage()).append('\n');
        }

        CreativeModeTabs.tryRebuildTabContents(FeatureFlags.DEFAULT_FLAGS, false, registries);
        for (CreativeModeTab tab : CreativeModeTabs.tabs()) {
            if (tab.getType() != CreativeModeTab.Type.CATEGORY) {
                continue;
            }
            String title = tab.getDisplayName().getContents() instanceof TranslatableContents t ? t.getKey() : "?";
            out.append("tab ").append(BuiltInRegistries.CREATIVE_MODE_TAB.getKey(tab).getPath())
                .append(' ').append(tab.row())
                .append(' ').append(tab.column())
                .append(' ').append(title)
                .append(' ').append(BuiltInRegistries.ITEM.getKey(tab.getIconItem().getItem()).getPath());
            List<String> seen = new ArrayList<>();
            for (ItemStack stack : tab.getDisplayItems()) {
                String id = BuiltInRegistries.ITEM.getKey(stack.getItem()).getPath();
                if (!seen.contains(id)) {
                    seen.add(id);
                    out.append(' ').append(id);
                }
            }
            out.append('\n');
        }

        // Bootstrap swaps System.out for a logger, which would prefix every line.
        new java.io.PrintStream(new java.io.FileOutputStream(java.io.FileDescriptor.out), true).print(out);
        System.exit(0);
    }

    /** Loads the vanilla datapack the way a server does, so tags and item components are bound. */
    private static HolderLookup.Provider loadVanilla() throws Exception {
        PackRepository packs = ServerPacksSource.createVanillaTrustedRepository();
        packs.reload();
        WorldDataConfiguration config = new WorldDataConfiguration(new DataPackConfig(List.of("vanilla"), List.of()), FeatureFlags.DEFAULT_FLAGS);
        WorldLoader.InitConfig init = new WorldLoader.InitConfig(
            new WorldLoader.PackConfig(packs, config, false, true), Commands.CommandSelection.DEDICATED, LevelBasedPermissionSet.OWNER
        );
        return Util.<HolderLookup.Provider>blockUntilDone(
                executor -> WorldLoader.load(
                    init,
                    context -> new WorldLoader.DataLoadOutput<Void>(null, context.datapackDimensions()),
                    (resources, managers, layers, cookie) -> {
                        managers.updateComponentsAndStaticRegistryTags();
                        return layers.compositeAccess();
                    },
                    Util.backgroundExecutor(),
                    executor
                )
            )
            .get();
    }
}
