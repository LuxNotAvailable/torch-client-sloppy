//! One plugin's sandbox: its `plugin.wasm` in wasmi, the host functions it
//! may call (`docs/MOD_API.md`, "Host functions"), and the calls into it.
//! Every call runs on a step budget (fuel); one that runs out or traps is
//! abandoned.

use wasmi::{Caller, Config, Engine, Instance, Linker, Memory, Module, Store, TypedFunc};

/// Steps one call may take: generous for building a rig, small enough that
/// a stuck plugin costs a frame, not the game.
const FUEL: u64 = 50_000_000;

/// The blocks a call may ask about (`block_at`): a copy of the world
/// around the block entity, by position, as state ids.
pub(super) type Near = std::collections::HashMap<[i32; 3], u16>;

/// How far from its block entity a plugin can see (`block_at`).
pub(super) const NEAR: i32 = 2;

struct HostState {
    /// The mod's id, for its log lines.
    name: String,
    /// What `block_at` answers from during the current call.
    near: Near,
}

pub(super) struct Runtime {
    store: Store<HostState>,
    memory: Memory,
    alloc: TypedFunc<i32, i32>,
    free: TypedFunc<(i32, i32), ()>,
    block_entity: Option<TypedFunc<(i32, i32), i64>>,
    block_tint: Option<TypedFunc<(i32, i32), i64>>,
    entity: Option<TypedFunc<(i32, i32), i64>>,
    payload: Option<TypedFunc<(i32, i32), i64>>,
    screen: Option<TypedFunc<(i32, i32), i64>>,
    item_tints: Option<TypedFunc<(i32, i32), i64>>,
    fluid: Option<TypedFunc<(i32, i32), i64>>,
    pub(super) registration: torch_mod_api::Registration,
}

fn mem_slice(memory: &Memory, store: impl wasmi::AsContext, ptr: i32, len: i32) -> Option<Vec<u8>> {
    let data = memory.data(&store);
    let start = usize::try_from(ptr).ok()?;
    let end = start.checked_add(usize::try_from(len).ok()?)?;
    data.get(start..end).map(<[u8]>::to_vec)
}

impl Runtime {
    /// Loads `wasm` and runs its `torch_init`. `name` tags its log lines.
    pub(super) fn load(name: &str, wasm: &[u8]) -> Result<Runtime, String> {
        let mut config = Config::default();
        config.consume_fuel(true);
        let engine = Engine::new(&config);
        let module = Module::new(&engine, wasm).map_err(|e| e.to_string())?;
        let mut store = Store::new(
            &engine,
            HostState {
                name: name.to_owned(),
                near: Near::new(),
            },
        );
        let mut linker = <Linker<HostState>>::new(&engine);
        linker
            .func_wrap(
                "torch",
                "log",
                |caller: Caller<'_, HostState>, ptr: i32, len: i32| {
                    let Some(memory) = caller.get_export("memory").and_then(|e| e.into_memory())
                    else {
                        return;
                    };
                    if let Some(bytes) = mem_slice(&memory, &caller, ptr, len) {
                        let name = &caller.data().name;
                        crate::log_info!("modapi", "{name}: {}", String::from_utf8_lossy(&bytes));
                    }
                },
            )
            .map_err(|e| e.to_string())?;
        linker
            .func_wrap(
                "torch",
                "block_at",
                |mut caller: Caller<'_, HostState>,
                 x: i32,
                 y: i32,
                 z: i32,
                 out: i32,
                 cap: i32|
                 -> i32 {
                    let answer = caller.data().near.get(&[x, y, z]).and_then(|&id| {
                        let state = azalea::block::BlockState::try_from(u32::from(id))
                            .ok()
                            .or_else(|| {
                                Some(azalea::block::BlockState::from_network(u32::from(id)))
                            })?;
                        let info = crate::blocks::info::info(state);
                        let block = if info.id.contains(':') {
                            info.id.to_owned()
                        } else {
                            format!("minecraft:{}", info.id)
                        };
                        Some(serde_json::json!({"block": block, "props": info.map()}).to_string())
                    });
                    let Some(answer) = answer else { return 0 };
                    let bytes = answer.as_bytes();
                    if bytes.len() > usize::try_from(cap).unwrap_or(0) {
                        return bytes.len() as i32;
                    }
                    let Some(memory) = caller.get_export("memory").and_then(|e| e.into_memory())
                    else {
                        return 0;
                    };
                    match memory.write(&mut caller, out.max(0) as usize, bytes) {
                        Ok(()) => bytes.len() as i32,
                        Err(_) => 0,
                    }
                },
            )
            .map_err(|e| e.to_string())?;
        store.set_fuel(FUEL).map_err(|e| e.to_string())?;
        let instance: Instance = linker
            .instantiate_and_start(&mut store, &module)
            .map_err(|e| e.to_string())?;
        let memory = instance
            .get_memory(&store, "memory")
            .ok_or("it exports no memory")?;
        let api = instance
            .get_typed_func::<(), i32>(&store, "torch_api")
            .map_err(|e| e.to_string())?
            .call(&mut store, ())
            .map_err(|e| e.to_string())?;
        if api > torch_mod_api::API_VERSION || api < 1 {
            return Err(format!(
                "it's built for mod API {api}; this client has {}",
                torch_mod_api::API_VERSION
            ));
        }
        fn missing(name: &'static str) -> impl Fn(wasmi::Error) -> String {
            move |e| format!("{name}: {e}")
        }
        let alloc = instance
            .get_typed_func::<i32, i32>(&store, "torch_alloc")
            .map_err(missing("torch_alloc"))?;
        let free = instance
            .get_typed_func::<(i32, i32), ()>(&store, "torch_free")
            .map_err(missing("torch_free"))?;
        let init = instance
            .get_typed_func::<(), i64>(&store, "torch_init")
            .map_err(missing("torch_init"))?;
        let block_entity = instance
            .get_typed_func::<(i32, i32), i64>(&store, "torch_block_entity")
            .ok();
        // Plugins built before the tint hook don't export it.
        let block_tint = instance
            .get_typed_func::<(i32, i32), i64>(&store, "torch_block_tint")
            .ok();
        let entity = instance
            .get_typed_func::<(i32, i32), i64>(&store, "torch_entity")
            .ok();
        let payload = instance
            .get_typed_func::<(i32, i32), i64>(&store, "torch_payload")
            .ok();
        let screen = instance
            .get_typed_func::<(i32, i32), i64>(&store, "torch_screen")
            .ok();
        let item_tints = instance
            .get_typed_func::<(i32, i32), i64>(&store, "torch_item_tints")
            .ok();
        let fluid = instance
            .get_typed_func::<(i32, i32), i64>(&store, "torch_fluid")
            .ok();
        let mut runtime = Runtime {
            store,
            memory,
            alloc,
            free,
            block_entity,
            block_tint,
            entity,
            payload,
            screen,
            item_tints,
            fluid,
            registration: torch_mod_api::Registration::default(),
        };
        let packed = runtime.call_fuelled(|store| init.call(store, ()))?;
        let bytes = runtime.take_result(packed)?;
        runtime.registration =
            serde_json::from_slice(&bytes).map_err(|e| format!("its registration: {e}"))?;
        Ok(runtime)
    }

    fn call_fuelled<R>(
        &mut self,
        call: impl FnOnce(&mut Store<HostState>) -> Result<R, wasmi::Error>,
    ) -> Result<R, String> {
        self.store.set_fuel(FUEL).map_err(|e| e.to_string())?;
        call(&mut self.store).map_err(|e| e.to_string())
    }

    /// The bytes of a `ptr << 32 | len` result, freed in the plugin.
    fn take_result(&mut self, packed: i64) -> Result<Vec<u8>, String> {
        let (ptr, len) = ((packed >> 32) as i32, packed as i32);
        let bytes =
            mem_slice(&self.memory, &self.store, ptr, len).ok_or("a result past its memory")?;
        let free = self.free;
        self.call_fuelled(|store| free.call(store, (ptr, len)))?;
        Ok(bytes)
    }

    /// Writes `input` into a buffer the plugin allocated.
    fn put(&mut self, input: &[u8]) -> Result<(i32, i32), String> {
        let len = i32::try_from(input.len()).map_err(|_| "input too large")?;
        let alloc = self.alloc;
        let ptr = self.call_fuelled(|store| alloc.call(store, len))?;
        self.memory
            .write(&mut self.store, ptr.max(0) as usize, input)
            .map_err(|e| e.to_string())?;
        Ok((ptr, len))
    }

    /// A mod packet on `channel`, for the plugin to keep.
    pub(super) fn payload(
        &mut self,
        channel: &str,
        data: &[u8],
    ) -> Result<torch_mod_api::Redraw, String> {
        let func = self.payload.ok_or("it has no torch_payload")?;
        let name = u16::try_from(channel.len()).map_err(|_| "channel name too long")?;
        let mut input = Vec::with_capacity(2 + channel.len() + data.len());
        input.extend(name.to_be_bytes());
        input.extend(channel.as_bytes());
        input.extend(data);
        let (ptr, len) = self.put(&input)?;
        let packed = self.call_fuelled(|store| func.call(store, (ptr, len)));
        let free = self.free;
        let _ = self.call_fuelled(|store| free.call(store, (ptr, len)));
        let bytes = self.take_result(packed?)?;
        serde_json::from_slice(&bytes).map_err(|e| format!("its redraw: {e}"))
    }

    /// An entity's rig, from its view as JSON.
    pub(super) fn entity(&mut self, view: &[u8]) -> Result<torch_mod_api::Rig, String> {
        let func = self.entity.ok_or("it has no torch_entity")?;
        let (ptr, len) = self.put(view)?;
        let packed = self.call_fuelled(|store| func.call(store, (ptr, len)));
        let free = self.free;
        let _ = self.call_fuelled(|store| free.call(store, (ptr, len)));
        let bytes = self.take_result(packed?)?;
        serde_json::from_slice(&bytes).map_err(|e| format!("its entity rig: {e}"))
    }

    /// A menu's screen, from its view as JSON; `None` for the client's own.
    pub(super) fn screen(&mut self, view: &[u8]) -> Result<Option<torch_mod_api::Screen>, String> {
        let func = self.screen.ok_or("it has no torch_screen")?;
        let (ptr, len) = self.put(view)?;
        let packed = self.call_fuelled(|store| func.call(store, (ptr, len)));
        let free = self.free;
        let _ = self.call_fuelled(|store| free.call(store, (ptr, len)));
        let bytes = self.take_result(packed?)?;
        serde_json::from_slice(&bytes).map_err(|e| format!("its screen: {e}"))
    }

    /// A fluid's look, from its view as JSON.
    pub(super) fn fluid(
        &mut self,
        view: &[u8],
    ) -> Result<Option<torch_mod_api::FluidLook>, String> {
        let func = self.fluid.ok_or("it has no torch_fluid")?;
        let (ptr, len) = self.put(view)?;
        let packed = self.call_fuelled(|store| func.call(store, (ptr, len)));
        let free = self.free;
        let _ = self.call_fuelled(|store| free.call(store, (ptr, len)));
        let bytes = self.take_result(packed?)?;
        serde_json::from_slice(&bytes).map_err(|e| format!("its fluid look: {e}"))
    }

    /// An item's tint colours, from its view as JSON.
    pub(super) fn item_tints(&mut self, view: &[u8]) -> Result<Vec<u32>, String> {
        let func = self.item_tints.ok_or("it has no torch_item_tints")?;
        let (ptr, len) = self.put(view)?;
        let packed = self.call_fuelled(|store| func.call(store, (ptr, len)));
        let free = self.free;
        let _ = self.call_fuelled(|store| free.call(store, (ptr, len)));
        let bytes = self.take_result(packed?)?;
        serde_json::from_slice(&bytes).map_err(|e| format!("its item tints: {e}"))
    }

    /// A block state's tint, from its view as JSON.
    pub(super) fn block_tint(&mut self, view: &[u8]) -> Result<torch_mod_api::Tint, String> {
        let func = self.block_tint.ok_or("it has no torch_block_tint")?;
        let (ptr, len) = self.put(view)?;
        let packed = self.call_fuelled(|store| func.call(store, (ptr, len)));
        let free = self.free;
        let _ = self.call_fuelled(|store| free.call(store, (ptr, len)));
        let bytes = self.take_result(packed?)?;
        serde_json::from_slice(&bytes).map_err(|e| format!("its tint: {e}"))
    }

    /// A block entity's rig, from its view as JSON. `near` answers the
    /// plugin's `block_at` calls.
    pub(super) fn block_entity(
        &mut self,
        view: &[u8],
        near: Near,
    ) -> Result<torch_mod_api::Rig, String> {
        let func = self.block_entity.ok_or("it has no torch_block_entity")?;
        let (ptr, len) = self.put(view)?;
        self.store.data_mut().near = near;
        let packed = self.call_fuelled(|store| func.call(store, (ptr, len)));
        self.store.data_mut().near.clear();
        let free = self.free;
        // The input buffer is the plugin's to free whether or not it worked.
        let _ = self.call_fuelled(|store| free.call(store, (ptr, len)));
        let bytes = self.take_result(packed?)?;
        serde_json::from_slice(&bytes).map_err(|e| format!("its rig: {e}"))
    }
}
