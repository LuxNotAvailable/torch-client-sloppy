//! GPU timestamps per pass (VULKAN_REWRITE_CHECKLIST.md 2.8): each frame in
//! flight has its own query pool; a slot's results are read when it comes
//! round again, after its fence, so reading never stalls.

use ash::vk;

use super::frame::FRAMES_IN_FLIGHT;
use super::{Context, SetupError};

const MAX_PASSES: u32 = 256;

pub struct Timestamps {
    pools: Vec<vk::QueryPool>,
    names: Vec<Vec<&'static str>>,
    slot: usize,
    period_ns: f32,
    /// The last finished frame's passes and their GPU milliseconds.
    pub last: Vec<(&'static str, f32)>,
}

impl Timestamps {
    pub fn new(ctx: &Context) -> Result<Timestamps, SetupError> {
        // SAFETY: plain queries and creation on a live device.
        let limits = unsafe { ctx.instance.get_physical_device_properties(ctx.physical) }.limits;
        let pools = (0..FRAMES_IN_FLIGHT)
            .map(|_| unsafe {
                let pool = ctx.device.create_query_pool(
                    &vk::QueryPoolCreateInfo::default()
                        .query_type(vk::QueryType::TIMESTAMP)
                        .query_count(MAX_PASSES * 2),
                    None,
                )?;
                ctx.device.reset_query_pool(pool, 0, MAX_PASSES * 2);
                Ok(pool)
            })
            .collect::<Result<Vec<_>, vk::Result>>()?;
        Ok(Timestamps {
            pools,
            names: vec![Vec::new(); FRAMES_IN_FLIGHT],
            slot: 0,
            period_ns: limits.timestamp_period,
            last: Vec::new(),
        })
    }

    /// Starts frame slot `slot`, after its fence: reads what it measured
    /// last time round, then resets its queries.
    pub fn begin_frame(&mut self, ctx: &Context, slot: usize) {
        self.slot = slot;
        let pool = self.pools[slot];
        let count = self.names[slot].len() as u32;
        if count > 0 {
            let mut ticks = vec![0u64; (count * 2) as usize];
            // SAFETY: the slot's fence has signalled, so its queries are
            // written; reading with no flags doesn't wait.
            let read = unsafe {
                ctx.device.get_query_pool_results(
                    pool,
                    0,
                    &mut ticks,
                    vk::QueryResultFlags::TYPE_64,
                )
            };
            if read.is_ok() {
                self.last = self.names[slot]
                    .iter()
                    .enumerate()
                    .map(|(i, &name)| {
                        let span = ticks[i * 2 + 1].saturating_sub(ticks[i * 2]);
                        (name, span as f32 * self.period_ns / 1e6)
                    })
                    .collect();
            }
        }
        // SAFETY: host reset (hostQueryReset is enabled) of queries the GPU
        // is done with.
        unsafe { ctx.device.reset_query_pool(pool, 0, MAX_PASSES * 2) };
        self.names[slot].clear();
    }

    pub fn begin(&mut self, ctx: &Context, commands: vk::CommandBuffer, name: &'static str) {
        let index = self.names[self.slot].len() as u32;
        if index >= MAX_PASSES {
            return;
        }
        self.names[self.slot].push(name);
        // SAFETY: a reset query in this slot's pool.
        unsafe {
            ctx.device.cmd_write_timestamp2(
                commands,
                vk::PipelineStageFlags2::TOP_OF_PIPE,
                self.pools[self.slot],
                index * 2,
            );
        }
    }

    pub fn end(&mut self, ctx: &Context, commands: vk::CommandBuffer) {
        let index = self.names[self.slot].len() as u32;
        if index == 0 || index > MAX_PASSES {
            return;
        }
        // SAFETY: the matching end query of the pass just begun.
        unsafe {
            ctx.device.cmd_write_timestamp2(
                commands,
                vk::PipelineStageFlags2::BOTTOM_OF_PIPE,
                self.pools[self.slot],
                (index - 1) * 2 + 1,
            );
        }
    }

    pub fn destroy(self, ctx: &Context) {
        // SAFETY: the caller idles the device first.
        unsafe {
            for pool in self.pools {
                ctx.device.destroy_query_pool(pool, None);
            }
        }
    }
}
