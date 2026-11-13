//! The render state layer under the shaders: Bethesda's render state setters with their lock
//! counters, the renderer's state cache that sends a value to the device only when it changed
//! (`NiDX9RenderState`), the texture stage caches, the sort of a shader constant map's entries, and
//! the geometry group hand-off.
//!
//! A device call stays a call through the device's vtable (`m_pkD3DDevice9`), which the platform layer
//! answers; what is typed is the bookkeeping around it.

use super::layout::{
    ConstantEntry, NiD3DShaderConstantMap, NiD3DTextureStage, NiDX9RenderState,
    NiGeometryBufferData, NiGeometryData, NiSkinInstance, NiSkinPartition, NiTArray, Partition,
    StageStateCache,
};
use super::{elem_addr, low8, FAMILY};
use crate::anim_keys::narrow;
use engine_core::ptr::{Field, Ptr};
use engine_core::typed::leaf;
use engine_core::{args, draw_side, typed_call, Engine};

/// The renderer (`011f9508`) whose state cache (+0x8b8) the setters use.
pub(crate) const RENDERER: u32 = 0x011f_9508;
/// The lock counters of the render states, by index (`011ff9d8`).
const LOCK_COUNTERS: u32 = 0x011f_f9d8;
/// The words that hold each setter's counter index.
const INDEX_Z_ENABLE: u32 = 0x011f_fa4c;
const INDEX_Z_WRITE: u32 = 0x011f_fa68;
const INDEX_ALPHA_TEST: u32 = 0x011f_fa6c;
const INDEX_SOURCE_BLEND: u32 = 0x011f_fa7c;
const INDEX_DESTINATION_BLEND: u32 = 0x011f_fa80;
const INDEX_Z_FUNCTION: u32 = 0x011f_fa8c;
const INDEX_ALPHA_REFERENCE: u32 = 0x011f_fa90;
const INDEX_ALPHA_FUNCTION: u32 = 0x011f_fa94;
const INDEX_ALPHA_BLEND: u32 = 0x011f_fa9c;
const INDEX_COLOR_WRITE: u32 = 0x011f_fcd0;
/// The counter index of each render state (`011ffa30`, by state).
const INDEX_BY_STATE: u32 = 0x011f_fa30;
/// The comparison functions (`011ad888`) and blend factors (`011ad8a8`) by Bethesda's enum.
const COMPARE_FUNCTIONS: u32 = 0x011a_d888;
const BLEND_FACTORS: u32 = 0x011a_d8a8;
/// The alpha-to-coverage counter (`011ffa28`) and which vendor's switch the card takes (`011f94b8`:
/// 1 the render state 0xb5 with `ATOC`, 2 the state 0x9a with `A2M0`/`A2M1`).
pub(crate) const COVERAGE_COUNTER: u32 = 0x011f_fa28;
const COVERAGE_KIND: u32 = 0x011f_94b8;

/// `SetRenderState` of the renderer's state cache, virtual slot 0x68: `thiscall(state cache, state,
/// value, 0, lock)` ([`set_render_state`]).
pub(crate) const SLOT_SET_RENDER_STATE: u32 = 0x68;
/// Device slots (`IDirect3DDevice9`): `SetRenderState`, `SetTexture`, `SetTextureStageState`,
/// `SetSamplerState`, `SetVertexDeclaration`, `SetVertexShader`, `SetPixelShader`.
const DEVICE_SET_RENDER_STATE: u32 = 0xe4;
const DEVICE_SET_TEXTURE: u32 = 0x104;
const DEVICE_SET_STAGE_STATE: u32 = 0x10c;
pub(crate) const DEVICE_SET_SAMPLER_STATE: u32 = 0x114;
const DEVICE_SET_DECLARATION: u32 = 0x15c;
const DEVICE_SET_VERTEX_SHADER: u32 = 0x170;
const DEVICE_SET_PIXEL_SHADER: u32 = 0x1ac;

/// Which cache slot a texture stage state (`0126f958`, 8 slots) and a sampler state (`0126f92c`, 5
/// slots) has, as 16-bit words by state; a state without one has 8 or 5 or more.
const STAGE_SLOT_OF_STATE: u32 = 0x0126_f958;
const SAMPLER_SLOT_OF_STATE: u32 = 0x0126_f92c;
/// The texture stage states and sampler states of the slots (`0126f948`, 16-bit; `0126f918`, words),
/// and the cache object they go to (`0126f99c`) with its slots 0xc0 (stage) and 0xcc (sampler).
const STAGE_STATE_OF_SLOT: u32 = 0x0126_f948;
const SAMPLER_STATE_OF_SLOT: u32 = 0x0126_f918;
const STATE_TARGET: u32 = 0x0126_f99c;
const SLOT_TARGET_STAGE_STATE: u32 = 0xc0;
const SLOT_TARGET_SAMPLER_STATE: u32 = 0xcc;
/// How many texture stages take stage states (`011bf558`).
const STAGE_COUNT: u32 = 0x011b_f558;
/// The address mode pairs (`0126f0b0`, two words an entry) and filter triples (`0126f050`, three).
const ADDRESS_MODES: u32 = 0x0126_f0b0;
const FILTER_MODES: u32 = 0x0126_f050;
/// The bounds of an alpha value the alpha test is off between (`0106d508`, a `double`; `0106b9e0`).
const ALPHA_LOW: u32 = 0x0106_d508;
const ALPHA_HIGH: u32 = 0x0106_b9e0;
/// `InterlockedIncrement` and `InterlockedDecrement`, their import slots.
const INTERLOCKED_INCREMENT: u32 = 0x00fd_f1c0;
const INTERLOCKED_DECREMENT: u32 = 0x00fd_f1c4;
/// `NiRefObject`'s deleting destructor, virtual slot 4 (no arguments).
const SLOT_DELETE: u32 = 0x4;
/// The null element of a pointer array (`0126f910`), made once (guard bit 0 of `0126f914`) with its
/// destructor (`00fde390`) handed to `atexit` (`00ec658f`).
const NULL_ELEMENT: u32 = 0x0126_f910;
const NULL_ELEMENT_GUARD: u32 = 0x0126_f914;
const NULL_ELEMENT_DESTRUCT: u32 = 0x00fd_e390;
const ATEXIT: u32 = 0x00ec_658f;
/// A geometry group's `AddObject`, virtual slot 8: `thiscall(group, data, skin, partition)`.
const SLOT_GROUP_ADD: u32 = 0x8;

/// The functions the render state cache's class (`NiDX9RenderState`, vtable `010f088c`) has in the
/// slots the typed code calls; a call through one of these slots that reaches the typed function is
/// a [`typed_call!`] ([`crate::slot_target`]).
const CACHE_SET_RENDER_STATE: u32 = 0x00e8_8780;
const CACHE_SET_STAGE_STATE: u32 = 0x00e8_8930;
const CACHE_SET_SAMPLER_STATE: u32 = 0x00e9_10a0;
const CACHE_SET_TEXTURE: u32 = 0x00e8_8a20;

/// `cache->SetRenderState(state, value, unused, lock)` (slot 0x68): [`set_render_state`] when the
/// slot holds it (and `lock` is a byte, all the typed function reads of the word), else through the
/// vtable.
#[inline(always)]
pub(crate) fn cache_render_state(
    e: &mut Engine,
    cache: u32,
    state: u32,
    value: u32,
    unused: u32,
    lock: u32,
) {
    if lock <= 0xff && crate::slot_target(e, cache, SLOT_SET_RENDER_STATE) == CACHE_SET_RENDER_STATE
    {
        typed_call!(
            FAMILY,
            0x00e8_8780,
            set_render_state(e, Ptr::new(cache), state, value, unused, low8(lock))
        );
    } else {
        e.vcall(
            cache,
            SLOT_SET_RENDER_STATE,
            &args![state, value, unused, lock],
        );
    }
}

/// `cache->SetTextureStageState(stage, state, value, unused)` (slot 0xc0): [`set_stage_state`] when
/// the slot holds it, else through the vtable.
#[inline(always)]
pub(crate) fn cache_stage_state(
    e: &mut Engine,
    cache: u32,
    stage: u32,
    state: u32,
    value: u32,
    unused: u32,
) {
    if crate::slot_target(e, cache, SLOT_TARGET_STAGE_STATE) == CACHE_SET_STAGE_STATE {
        typed_call!(
            FAMILY,
            0x00e8_8930,
            set_stage_state(e, Ptr::new(cache), stage, state, value, unused)
        );
    } else {
        e.vcall(
            cache,
            SLOT_TARGET_STAGE_STATE,
            &args![stage, state, value, unused],
        );
    }
}

/// `cache->SetSamplerState(stage, state, value, unused)` (slot 0xcc): [`set_sampler_state`] when the
/// slot holds it, else through the vtable.
#[inline(always)]
pub(crate) fn cache_sampler_state(
    e: &mut Engine,
    cache: u32,
    stage: u32,
    state: u32,
    value: u32,
    unused: u32,
) {
    if crate::slot_target(e, cache, SLOT_TARGET_SAMPLER_STATE) == CACHE_SET_SAMPLER_STATE {
        typed_call!(
            FAMILY,
            0x00e9_10a0,
            set_sampler_state(e, Ptr::new(cache), stage, state, value, unused)
        );
    } else {
        e.vcall(
            cache,
            SLOT_TARGET_SAMPLER_STATE,
            &args![stage, state, value, unused],
        );
    }
}

/// `cache->SetTexture(stage, texture)` (slot 0xdc): [`set_texture`] when the slot holds it, else
/// through the vtable.
#[inline(always)]
pub(crate) fn cache_texture(e: &mut Engine, cache: u32, stage: u32, texture: u32) {
    if crate::slot_target(e, cache, SLOT_CACHE_TEXTURE) == CACHE_SET_TEXTURE {
        typed_call!(
            FAMILY,
            0x00e8_8a20,
            set_texture(e, Ptr::new(cache), stage, texture)
        );
    } else {
        e.vcall(cache, SLOT_CACHE_TEXTURE, &args![stage, texture]);
    }
}

/// The cache's `SetTexture` slot.
const SLOT_CACHE_TEXTURE: u32 = 0xdc;

/// A field of the state cache, read or written through the drawing side's copy of the render layer's shared
/// state (`engine_core::draw_side`): the drawing side's own while a side is open on its thread, guest memory
/// (main's change noted while one is open) otherwise.
#[inline(always)]
fn cache_u32(e: &Engine, this: Ptr<NiDX9RenderState>, field: Field<NiDX9RenderState, u32>) -> u32 {
    draw_side::load_u32(e, this.addr().wrapping_add(field.off))
}
#[inline(always)]
fn set_cache_u32(
    e: &Engine,
    this: Ptr<NiDX9RenderState>,
    field: Field<NiDX9RenderState, u32>,
    value: u32,
) {
    draw_side::store_u32(e, this.addr().wrapping_add(field.off), value);
}
#[inline(always)]
fn cache_u8(e: &Engine, this: Ptr<NiDX9RenderState>, field: Field<NiDX9RenderState, u8>) -> u8 {
    draw_side::load_u8(e, this.addr().wrapping_add(field.off))
}
#[inline(always)]
fn set_cache_u8(
    e: &Engine,
    this: Ptr<NiDX9RenderState>,
    field: Field<NiDX9RenderState, u8>,
    value: u8,
) {
    draw_side::store_u8(e, this.addr().wrapping_add(field.off), value);
}
#[inline(always)]
fn set_cache_f32(
    e: &Engine,
    this: Ptr<NiDX9RenderState>,
    field: Field<NiDX9RenderState, f32>,
    value: f32,
) {
    draw_side::store_f32(e, this.addr().wrapping_add(field.off), value);
}

/// The ranges of the render layer's shared state the drawing side keeps its own copy of
/// (`engine_core::draw_side::open` at the cut): the renderer's state cache (`NiDX9RenderState`, 0x1114 bytes),
/// Bethesda's render state lock counters (`011ff9d8`, up to the alpha-to-coverage counter `011ffa28`, which ends
/// them), merged as balanced counters. Empty without a renderer.
pub fn side_ranges(e: &Engine) -> Vec<(u32, u32, engine_core::draw_side::Kind)> {
    use engine_core::draw_side::Kind;
    let cache = render_state(e).addr();
    if cache == 0 {
        return Vec::new();
    }
    vec![
        (cache, CACHE_SIZE, Kind::Device),
        (
            LOCK_COUNTERS,
            COVERAGE_COUNTER + 4 - LOCK_COUNTERS,
            Kind::Balanced,
        ),
    ]
}

/// `NiDX9RenderState`'s size.
const CACHE_SIZE: u32 = 0x1114;

/// The renderer's state cache.
#[inline(always)]
pub(crate) fn render_state(e: &Engine) -> Ptr<NiDX9RenderState> {
    let renderer = e.global::<u32>(RENDERER);
    Ptr::new(e.global::<u32>(renderer.wrapping_add(0x8b8)))
}

/// The pattern of every lock-counted setter: the cache gets `state = value` while the counter at
/// `index_at`'s index is 0; the counter then moves by `lock` either way.
#[inline(always)]
fn counted(e: &mut Engine, index_at: u32, state: u32, value: u32, lock: u32) {
    let counter = LOCK_COUNTERS.wrapping_add(e.global::<u32>(index_at).wrapping_mul(4));
    if draw_side::load_u32(e, counter) == 0 {
        let cache = render_state(e);
        cache_render_state(e, cache.addr(), state, value, 0, 0);
    }
    let count = draw_side::load_u32(e, counter).wrapping_add(lock);
    draw_side::store_u32(e, counter, count);
}

// Typed from 00b97de0
/// `cdecl(enable, lock)`: `D3DRS_ZENABLE` (7).
pub fn set_z_enable(e: &mut Engine, enable: u8, lock: u32) {
    counted(e, INDEX_Z_ENABLE, 7, u32::from(enable), lock);
}

// Typed from 00b97e30
/// `BSRenderState::SetZWriteEnable` (engine map), `cdecl(enable, lock)`: `D3DRS_ZWRITEENABLE` (0xe).
pub fn set_z_write_enable(e: &mut Engine, enable: u8, lock: u32) {
    counted(e, INDEX_Z_WRITE, 0xe, u32::from(enable), lock);
}

// Typed from 00b97ed0
/// `cdecl(enable, lock)`: `D3DRS_ALPHATESTENABLE` (0xf).
pub fn set_alpha_test_enable(e: &mut Engine, enable: u8, lock: u32) {
    counted(e, INDEX_ALPHA_TEST, 0xf, u32::from(enable), lock);
}

// Typed from 00b97fa0
/// `BSRenderStateManager::SetDepthBufferEnabled` (engine map), `cdecl(enable, lock)`:
/// `D3DRS_ALPHABLENDENABLE` (0x1b).
pub fn set_alpha_blend_enable(e: &mut Engine, enable: u8, lock: u32) {
    counted(e, INDEX_ALPHA_BLEND, 0x1b, u32::from(enable), lock);
}

// Typed from 00b98380
/// `BSRenderStateManager::SetColorWriteMask` (engine map), `cdecl(mask, lock)`:
/// `D3DRS_COLORWRITEENABLE` (0xa8).
pub fn set_color_write(e: &mut Engine, mask: u32, lock: u32) {
    counted(e, INDEX_COLOR_WRITE, 0xa8, mask, lock);
}

// Typed from 00b97e80
/// `cdecl(function, lock)`: `D3DRS_ZFUNC` (0x17), Bethesda's function enum through the table.
pub fn set_z_function(e: &mut Engine, function: u32, lock: u32) {
    let value = e.global::<u32>(COMPARE_FUNCTIONS.wrapping_add(function.wrapping_mul(4)));
    counted(e, INDEX_Z_FUNCTION, 0x17, value, lock);
}

// Typed from 00b97f20
/// `cdecl(function, reference, lock)`: `D3DRS_ALPHAFUNC` (0x19, through the table), then
/// `D3DRS_ALPHAREF` (0x18); each counter moves by `lock`.
pub fn set_alpha_function(e: &mut Engine, function: u32, reference: u32, lock: u32) {
    let value = e.global::<u32>(COMPARE_FUNCTIONS.wrapping_add(function.wrapping_mul(4)));
    counted(e, INDEX_ALPHA_FUNCTION, 0x19, value, lock);
    counted(e, INDEX_ALPHA_REFERENCE, 0x18, reference, lock);
}

// Typed from 00b97ff0
/// `cdecl(source, destination, lock)`: `D3DRS_SRCBLEND` (0x13) and `D3DRS_DESTBLEND` (0x14), both
/// through the blend table.
pub fn set_blend(e: &mut Engine, source: u32, destination: u32, lock: u32) {
    let value = e.global::<u32>(BLEND_FACTORS.wrapping_add(source.wrapping_mul(4)));
    counted(e, INDEX_SOURCE_BLEND, 0x13, value, lock);
    let value = e.global::<u32>(BLEND_FACTORS.wrapping_add(destination.wrapping_mul(4)));
    counted(e, INDEX_DESTINATION_BLEND, 0x14, value, lock);
}

// Typed from 00b97d90
/// `cdecl(state, value, lock)`: any render state, its counter index from the table by state.
pub fn set_state(e: &mut Engine, state: u32, value: u32, lock: u32) {
    let index_at = INDEX_BY_STATE.wrapping_add(state.wrapping_mul(4));
    counted(e, index_at, state, value, lock);
}

// Typed from 00b98540
/// `cdecl(enable, lock)`: alpha to coverage, through the vendor's render state. Only while its counter
/// is 0, and only for a card that has one (kind 1: state 0xb5 takes `ATOC` or 0.0; kind 2: state 0x9a
/// takes `A2M1` or `A2M0`); the counter moves by `lock` either way.
pub fn set_alpha_to_coverage(e: &mut Engine, enable: u8, lock: u32) {
    let count = draw_side::load_u32(e, COVERAGE_COUNTER);
    if count == 0 {
        let kind = e.global::<u32>(COVERAGE_KIND);
        let call = match kind {
            // `FLDZ; FSTP`: positive zero under any control word.
            1 => Some((0xb5u32, if enable != 0 { 0x434f_5441 } else { 0 })),
            2 => Some((
                0x9a,
                0x304d_3241u32.wrapping_add(if enable != 0 { 0x0100_0000 } else { 0 }),
            )),
            _ => None,
        };
        if let Some((state, value)) = call {
            let cache = render_state(e);
            cache_render_state(e, cache.addr(), state, value, 0, 0);
            let count = draw_side::load_u32(e, COVERAGE_COUNTER).wrapping_add(lock);
            draw_side::store_u32(e, COVERAGE_COUNTER, count);
            return;
        }
    }
    draw_side::store_u32(e, COVERAGE_COUNTER, count.wrapping_add(lock));
}

// Typed from 00b984d0
/// `cdecl(index, unused)`: the cull mode `index` of the renderer's state cache ([`set_cull_mode`]).
pub fn set_cull_mode_of_renderer(e: &mut Engine, index: u32, _unused: u32) {
    let cache = render_state(e);
    typed_call!(FAMILY, 0x00e8_79b0, set_cull_mode(e, cache, index, 0u32));
}

// Typed from 00e879b0
/// `thiscall(cache, index, lock)`: `D3DRS_CULLMODE` (0x16) through the cache's own `SetRenderState`,
/// the mode `index` of the pair the cull side (+0xf4) picks.
pub fn set_cull_mode(e: &mut Engine, this: Ptr<NiDX9RenderState>, index: u32, lock: u32) {
    let side = cache_u32(e, this, NiDX9RenderState::uiCullSide);
    let value = draw_side::load_u32(
        e,
        elem_addr(
            this,
            NiDX9RenderState::auiCullModes,
            side.wrapping_add(index.wrapping_mul(2)),
            4,
        ),
    );
    cache_render_state(e, this.addr(), 0x16, value, 0, lock);
}

// Typed from 00e88780
/// `NiDX9RenderState::SetRenderState` (Gamebryo's name), virtual slot 0x68, `thiscall(cache, state,
/// value, unused, lock)`: nothing while the state is locked; else the lock byte is stored, and the
/// device gets the value when it differs from the cached one.
pub fn set_render_state(
    e: &mut Engine,
    this: Ptr<NiDX9RenderState>,
    state: u32,
    value: u32,
    _unused: u32,
    lock: u8,
) {
    let locked = elem_addr(this, NiDX9RenderState::abLocked, state, 1);
    if draw_side::load_u8(e, locked) != 0 {
        return;
    }
    draw_side::store_u8(e, locked, lock);
    let cached = elem_addr(this, NiDX9RenderState::auiRenderStates, state, 8);
    if draw_side::load_u32(e, cached) != value {
        let device = cache_u32(e, this, NiDX9RenderState::m_pkD3DDevice9);
        e.vcall(device, DEVICE_SET_RENDER_STATE, &args![state, value]);
        draw_side::store_u32(e, cached, value);
    }
}

/// Slot `table[state]` (16-bit) when it is below `slots`.
#[inline(always)]
fn slot_of(e: &Engine, table: u32, state: u32, slots: u16) -> Option<u32> {
    let slot = e.global::<u16>(table.wrapping_add(state.wrapping_mul(2)));
    (slot < slots).then_some(u32::from(slot))
}

// Typed from 00e88930
/// `thiscall(cache, stage, state, value, unused)`: a texture stage state, sent to the device
/// (`SetTextureStageState`) when it differs from the cached one; a state without a slot does nothing.
pub fn set_stage_state(
    e: &mut Engine,
    this: Ptr<NiDX9RenderState>,
    stage: u32,
    state: u32,
    value: u32,
    _unused: u32,
) {
    let Some(slot) = slot_of(e, STAGE_SLOT_OF_STATE, state, 8) else {
        return;
    };
    let cached = elem_addr(
        this,
        NiDX9RenderState::auiStageStates,
        slot.wrapping_add(stage.wrapping_mul(8)),
        8,
    );
    if draw_side::load_u32(e, cached) != value {
        let device = cache_u32(e, this, NiDX9RenderState::m_pkD3DDevice9);
        e.vcall(device, DEVICE_SET_STAGE_STATE, &args![stage, state, value]);
    }
    draw_side::store_u32(e, cached, value);
}

// Typed from 00e910a0
/// `thiscall(cache, stage, state, value, unused)`: a sampler state, cached then sent
/// (`SetSamplerState`) when it differs; a state without a slot does nothing.
pub fn set_sampler_state(
    e: &mut Engine,
    this: Ptr<NiDX9RenderState>,
    stage: u32,
    state: u32,
    value: u32,
    _unused: u32,
) {
    let Some(slot) = slot_of(e, SAMPLER_SLOT_OF_STATE, state, 5) else {
        return;
    };
    let cached = elem_addr(
        this,
        NiDX9RenderState::auiSamplerStates,
        stage.wrapping_mul(5).wrapping_add(slot),
        8,
    );
    if draw_side::load_u32(e, cached) == value {
        return;
    }
    draw_side::store_u32(e, cached, value);
    let device = cache_u32(e, this, NiDX9RenderState::m_pkD3DDevice9);
    e.vcall(
        device,
        DEVICE_SET_SAMPLER_STATE,
        &args![stage, state, value],
    );
}

// Typed from 00e88a20
/// `thiscall(cache, stage, texture)`: the stage's texture, cached then sent (`SetTexture`) when it
/// differs.
pub fn set_texture(e: &mut Engine, this: Ptr<NiDX9RenderState>, stage: u32, texture: u32) {
    let cached = elem_addr(this, NiDX9RenderState::apTextures, stage, 4);
    if draw_side::load_u32(e, cached) == texture {
        return;
    }
    draw_side::store_u32(e, cached, texture);
    let device = cache_u32(e, this, NiDX9RenderState::m_pkD3DDevice9);
    e.vcall(device, DEVICE_SET_TEXTURE, &args![stage, texture]);
}

/// The shader and declaration setters' pattern: `saved` keeps the current one when asked; a new one
/// is cached and sent (the exe jumps to the device's slot).
#[inline(always)]
fn set_cached(
    e: &mut Engine,
    this: Ptr<NiDX9RenderState>,
    current: Field<NiDX9RenderState, u32>,
    saved: Field<NiDX9RenderState, u32>,
    slot: u32,
    value: u32,
    keep: u8,
) {
    if keep != 0 {
        let now = cache_u32(e, this, current);
        set_cache_u32(e, this, saved, now);
    }
    if cache_u32(e, this, current) == value {
        return;
    }
    set_cache_u32(e, this, current, value);
    let device = cache_u32(e, this, NiDX9RenderState::m_pkD3DDevice9);
    e.vcall(device, slot, &args![value]);
}

// Typed from 00e90e00
/// `thiscall(cache, shader, keep)`: the vertex shader (`SetVertexShader`).
pub fn set_vertex_shader(e: &mut Engine, this: Ptr<NiDX9RenderState>, shader: u32, keep: u8) {
    set_cached(
        e,
        this,
        NiDX9RenderState::pVertexShader,
        NiDX9RenderState::pSavedVertexShader,
        DEVICE_SET_VERTEX_SHADER,
        shader,
        keep,
    );
}

// Typed from 00e88870
/// `thiscall(cache, shader, keep)`: the pixel shader (`SetPixelShader`).
pub fn set_pixel_shader(e: &mut Engine, this: Ptr<NiDX9RenderState>, shader: u32, keep: u8) {
    set_cached(
        e,
        this,
        NiDX9RenderState::pPixelShader,
        NiDX9RenderState::pSavedPixelShader,
        DEVICE_SET_PIXEL_SHADER,
        shader,
        keep,
    );
}

// Typed from 00e90fc0
/// `thiscall(cache, declaration, keep)`: the vertex declaration (`SetVertexDeclaration`). A null one
/// does nothing; the same one again does nothing once one is set. Else the set byte goes on, the
/// current one is saved when asked, and the new one is cached and sent.
pub fn set_declaration(e: &mut Engine, this: Ptr<NiDX9RenderState>, declaration: u32, keep: u8) {
    if declaration == 0 {
        return;
    }
    if cache_u8(e, this, NiDX9RenderState::ucDeclarationSet) != 0
        && cache_u32(e, this, NiDX9RenderState::pDeclaration) == declaration
    {
        return;
    }
    set_cache_u8(e, this, NiDX9RenderState::ucDeclarationSet, 1);
    if keep != 0 {
        let now = cache_u32(e, this, NiDX9RenderState::pDeclaration);
        set_cache_u32(e, this, NiDX9RenderState::pSavedDeclaration, now);
    }
    set_cache_u32(e, this, NiDX9RenderState::pDeclaration, declaration);
    let device = cache_u32(e, this, NiDX9RenderState::m_pkD3DDevice9);
    e.vcall(device, DEVICE_SET_DECLARATION, &args![declaration]);
}

// Typed from 00e879e0
/// `thiscall(cache, object)`: the alpha test (`D3DRS_ALPHATESTENABLE`, 0x8f in Gamebryo's enum,
/// through the cache's `SetRenderState`) goes off when the object's alpha (+0x30) lies within the two
/// bounds (both ordered) and neither force byte is set; on otherwise. Comparisons are exact under any
/// control word.
pub fn set_alpha_test_of(e: &mut Engine, this: Ptr<NiDX9RenderState>, object: u32) {
    let alpha = f64::from(e.global::<f32>(object.wrapping_add(0x30)));
    let low = e.global::<f64>(ALPHA_LOW);
    let high = f64::from(e.global::<f32>(ALPHA_HIGH));
    let off = alpha >= low
        && high >= alpha
        && cache_u8(e, this, NiDX9RenderState::ucForceAlphaTestB) == 0
        && cache_u8(e, this, NiDX9RenderState::ucForceAlphaTestA) == 0;
    cache_render_state(e, this.addr(), 0x8f, u32::from(!off), 0, 0);
}

// Typed from 00e88730
/// `thiscall(cache, first, second)`: both floats stored, and second minus first (`f64`, rounded where
/// the exe stores it: this also runs on another game thread).
pub fn set_range(e: &mut Engine, this: Ptr<NiDX9RenderState>, first: f32, second: f32) {
    set_cache_f32(e, this, NiDX9RenderState::fRangeFirst, first);
    set_cache_f32(e, this, NiDX9RenderState::fRangeSecond, second);
    let difference = narrow(f64::from(second) - f64::from(first));
    set_cache_f32(e, this, NiDX9RenderState::fRangeDifference, difference);
}

// Typed from 00e88750
/// `thiscall(cache)`: whether the cull side (+0xf4) is set.
pub fn cull_side_set(e: &mut Engine, this: Ptr<NiDX9RenderState>) -> bool {
    cache_u32(e, this, NiDX9RenderState::uiCullSide) != 0
}

// Typed from 00e89060
/// `thiscall(cache, state, value out, bank out)`: a texture stage state's cached value, from bank B
/// (bank byte 0) before bank A (1); false when neither has it or the state has no slot.
pub fn cached_stage_state(
    e: &mut Engine,
    this: Ptr<StageStateCache>,
    state: u32,
    value: u32,
    bank: u32,
) -> bool {
    let Some(slot) = slot_of(e, STAGE_SLOT_OF_STATE, state, 8) else {
        return false;
    };
    type C = StageStateCache;
    for (set, values, which) in [
        (C::abSetB, C::auiValuesB, 0u8),
        (C::abSetA, C::auiValuesA, 1),
    ] {
        if e.global::<u8>(elem_addr(this, set, slot, 1)) != 0 {
            let cached = e.global::<u32>(elem_addr(this, values, slot, 4));
            e.set_global(value, cached);
            e.set_global(bank, which);
            return true;
        }
    }
    false
}

// Typed from 00e89250
/// `thiscall(cache, stage)`: hands each of the eight stage-state slots that has a value (bank B
/// before A) to the state target's slot 0xc0 with the stage and the slot's state. Answers 0.
pub fn apply_stage_states(e: &mut Engine, this: Ptr<StageStateCache>, stage: u32) -> u32 {
    type C = StageStateCache;
    for slot in 0..8 {
        let value = if e.global::<u8>(elem_addr(this, C::abSetB, slot, 1)) != 0 {
            e.global::<u32>(elem_addr(this, C::auiValuesB, slot, 4))
        } else if e.global::<u8>(elem_addr(this, C::abSetA, slot, 1)) != 0 {
            e.global::<u32>(elem_addr(this, C::auiValuesA, slot, 4))
        } else {
            continue;
        };
        let target = e.global::<u32>(STATE_TARGET);
        let state = u32::from(e.global::<u16>(STAGE_STATE_OF_SLOT.wrapping_add(slot * 2)));
        cache_stage_state(e, target, stage, state, value, 0);
    }
    0
}

// Typed from 00e892d0
/// `thiscall(cache, stage)`: the same for the five sampler slots (bank D before C) through slot 0xcc.
/// Answers 0.
pub fn apply_sampler_states(e: &mut Engine, this: Ptr<StageStateCache>, stage: u32) -> u32 {
    type C = StageStateCache;
    for slot in 0..5 {
        let value = if e.global::<u8>(elem_addr(this, C::abSetD, slot, 1)) != 0 {
            e.global::<u32>(elem_addr(this, C::auiValuesD, slot, 4))
        } else if e.global::<u8>(elem_addr(this, C::abSetC, slot, 1)) != 0 {
            e.global::<u32>(elem_addr(this, C::auiValuesC, slot, 4))
        } else {
            continue;
        };
        let target = e.global::<u32>(STATE_TARGET);
        let state = e.global::<u32>(SAMPLER_STATE_OF_SLOT.wrapping_add(slot * 4));
        cache_sampler_state(e, target, stage, state, value, 0);
    }
    0
}

// Typed from 00e89410
/// `thiscall(cache, stage)`: the stage states (only for a stage below the stage count) and the
/// sampler states of a stage; answers the first non-zero answer (both are 0).
pub fn apply_states(e: &mut Engine, this: Ptr<StageStateCache>, stage: u32) -> u32 {
    let mut first = 0;
    if stage < e.global::<u32>(STAGE_COUNT) {
        first = typed_call!(FAMILY, 0x00e8_9250, apply_stage_states(e, this, stage));
    }
    let second = typed_call!(FAMILY, 0x00e8_92d0, apply_sampler_states(e, this, stage));
    if first != 0 {
        first
    } else {
        second
    }
}

// Typed from 00e7de90
/// `thiscall(stage, index)`: the address mode pair `index` (U then V, sampler states 1 and 2) into the
/// stage's cache, bank D.
pub fn set_address_modes(e: &mut Engine, this: Ptr<NiD3DTextureStage>, index: u32) {
    let entry = ADDRESS_MODES.wrapping_add(index.wrapping_mul(8));
    for (state, at) in [(1u32, entry), (2, entry.wrapping_add(4))] {
        let value = e.global::<u32>(at);
        let cache = e.get(this, NiD3DTextureStage::pStateCache);
        typed_call!(
            leaf::FAMILY,
            0x00e8_90c0,
            leaf::set_state_slot(e, cache.addr(), state, value, 0u32, 0u32)
        );
    }
}

// Typed from 00e7def0
/// `NiD3DTextureStage::SetFilterMode` (engine map), `thiscall(stage, mode)`: the filter triple `mode`
/// into the stage's cache, bank D: sampler state 5 (magnification) takes the triple's second word, 6
/// (minification) its first, 7 (mip) its third. The mode is kept at +0.
pub fn set_filter_mode(e: &mut Engine, this: Ptr<NiD3DTextureStage>, mode: u32) {
    let triple = FILTER_MODES.wrapping_add(mode.wrapping_mul(12));
    for (state, word) in [(5u32, 1u32), (6, 0), (7, 2)] {
        let value = e.global::<u32>(triple.wrapping_add(word * 4));
        let cache = e.get(this, NiD3DTextureStage::pStateCache);
        typed_call!(
            leaf::FAMILY,
            0x00e8_90c0,
            leaf::set_state_slot(e, cache.addr(), state, value, 0u32, 0u32)
        );
    }
    e.set(this, NiD3DTextureStage::m_eFilterMode, mode);
}

// Typed from 00e8eeb0
/// `thiscall(buffer data)`: whether there are streams and every one has its buffer (+8).
pub fn has_streams(e: &mut Engine, this: Ptr<NiGeometryBufferData>) -> bool {
    let count = e.get(this, NiGeometryBufferData::m_uiStreamCount);
    if count == 0 {
        return false;
    }
    let streams = e.get(this, NiGeometryBufferData::m_ppkVBInfo);
    (0..count).all(|i| {
        let stream = e.global::<u32>(streams.wrapping_add(i * 4));
        stream != 0 && e.global::<u32>(stream.wrapping_add(8)) != 0
    })
}

// Typed from 00e88dc0
/// `NiD3DGeometryGroupManager::AddObjectToGroup` (engine map), `thiscall(manager, group, data, skin,
/// partitions, unused, unused)`: with a skin and `partitions`, each partition of the skin's partition
/// data goes to the group (slot 8), unless the first already has a buffer (false); otherwise the data
/// goes once, unless it already has a buffer (false).
#[allow(clippy::too_many_arguments)]
pub fn add_object_to_group(
    e: &mut Engine,
    _this: u32,
    group: u32,
    data: Ptr<NiGeometryData>,
    skin: Ptr<NiSkinInstance>,
    partitions: u8,
    _unused_5: u32,
    _unused_6: u32,
) -> bool {
    if !skin.is_null() && partitions != 0 {
        let partition_data = e.get(skin, NiSkinInstance::m_spSkinPartition);
        if !partition_data.is_null() {
            let mut partition =
                Ptr::<Partition>::new(e.get(partition_data, NiSkinPartition::m_pkPartitions));
            if e.get(partition, Partition::m_pkBuffData) != 0 {
                return false;
            }
            let count = e.get(partition_data, NiSkinPartition::m_uiPartitions);
            for _ in 0..count {
                e.vcall(group, SLOT_GROUP_ADD, &args![data, skin, partition]);
                partition = partition.index(1, 0x2c);
            }
            return true;
        }
    }
    if e.get(data, NiGeometryData::m_pkBuffData) != 0 {
        return false;
    }
    e.vcall(group, SLOT_GROUP_ADD, &args![data, skin, 0u32]);
    true
}

/// `InterlockedDecrement` of an object's count, and its deleting destructor at 0.
#[inline(always)]
fn release(e: &mut Engine, object: u32) {
    if e.call(INTERLOCKED_DECREMENT, &args![object.wrapping_add(4)])
        .u32()
        == 0
    {
        e.vcall(object, SLOT_DELETE, &[]);
    }
}

#[inline(always)]
fn hold(e: &mut Engine, object: u32) {
    e.call(INTERLOCKED_INCREMENT, &args![object.wrapping_add(4)]);
}

// Typed from 00e86f40
/// `NiTArray::SetAt` (Xbox PDB) for `NiPointer`s, `thiscall(array, index, element pointer)`: past the
/// end the size grows to `index + 1` and the count of non-null entries counts a non-null element;
/// within, the count follows null to non-null and back. The slot then takes the element with the
/// references moved (the old one released, the new one held), unless it is the same. The null element
/// is made on first use.
pub fn array_set_at(e: &mut Engine, this: Ptr<NiTArray>, index: u32, element: u32) {
    let guard = e.global::<u32>(NULL_ELEMENT_GUARD);
    if guard & 1 == 0 {
        e.set_global(NULL_ELEMENT_GUARD, guard | 1);
        e.set_global(NULL_ELEMENT, 0u32);
        e.call(ATEXIT, &args![NULL_ELEMENT_DESTRUCT]);
    }
    let size = u32::from(e.get(this, NiTArray::m_usSize));
    let null = e.global::<u32>(NULL_ELEMENT);
    let delta: u16 = if index >= size {
        e.set(this, NiTArray::m_usSize, low16(index.wrapping_add(1)));
        u16::from(e.global::<u32>(element) != null)
    } else {
        let current = e.global::<u32>(
            e.get(this, NiTArray::m_pBase)
                .wrapping_add(index.wrapping_mul(4)),
        );
        match (e.global::<u32>(element) == null, current == null) {
            (false, true) => 1,
            (true, false) => 0xffff,
            _ => 0,
        }
    };
    if delta != 0 {
        let count = e.get(this, NiTArray::m_usESize).wrapping_add(delta);
        e.set(this, NiTArray::m_usESize, count);
    }
    let slot = e
        .get(this, NiTArray::m_pBase)
        .wrapping_add(index.wrapping_mul(4));
    let old = e.global::<u32>(slot);
    if old == e.global::<u32>(element) {
        return;
    }
    if old != 0 {
        release(e, old);
    }
    let new = e.global::<u32>(element);
    e.set_global(slot, new);
    if new != 0 {
        hold(e, new);
    }
}

/// The low 16 bits (`MOV word`).
#[inline(always)]
#[allow(clippy::cast_possible_truncation)]
fn low16(word: u32) -> u16 {
    word as u16
}

// Typed from 00e870c0
/// `thiscall(map, array)`: moves the enabled entries (byte +8 equal to 1) of a constant map's array
/// ahead of the disabled ones (byte 0), keeping each group's order: the first disabled position found
/// takes the next enabled entry by two `SetAt`s through a held local, as the exe does.
pub fn sort_entries_by_enabled(e: &mut Engine, _this: u32, array: Ptr<NiTArray>) {
    let size = u32::from(e.get(array, NiTArray::m_usSize));
    if size <= 1 {
        return;
    }
    e.with_stack(4, |e, local| {
        let held_slot = local.addr();
        let mut held = 0u32;
        e.set_global(held_slot, held);
        // Takes slot `index` into the held local, the references moved; false when it is null.
        let take = |e: &mut Engine, held: &mut u32, index: u32| -> bool {
            let slot = e
                .get(array, NiTArray::m_pBase)
                .wrapping_add(index.wrapping_mul(4));
            if *held != e.global::<u32>(slot) {
                if *held != 0 {
                    release(e, *held);
                }
                *held = e.global::<u32>(slot);
                e.set_global(held_slot, *held);
                if *held == 0 {
                    return false;
                }
                hold(e, *held);
            }
            *held != 0
        };
        let mut disabled = u32::MAX;
        let mut index = 0u32;
        loop {
            if take(e, &mut held, index) {
                let entry = Ptr::<ConstantEntry>::new(held);
                if e.get(entry, ConstantEntry::ucEnabled) == 1 {
                    if disabled < index {
                        let other = e
                            .get(array, NiTArray::m_pBase)
                            .wrapping_add(disabled.wrapping_mul(4));
                        typed_call!(FAMILY, 0x00e8_6f40, array_set_at(e, array, index, other));
                        typed_call!(
                            FAMILY,
                            0x00e8_6f40,
                            array_set_at(e, array, disabled, held_slot)
                        );
                        disabled = disabled.wrapping_add(1);
                        if disabled >= size {
                            release(e, held);
                            return;
                        }
                        loop {
                            let slot = e
                                .get(array, NiTArray::m_pBase)
                                .wrapping_add(disabled.wrapping_mul(4));
                            if held != e.global::<u32>(slot) {
                                release(e, held);
                                held = e.global::<u32>(slot);
                                e.set_global(held_slot, held);
                                if held != 0 {
                                    hold(e, held);
                                }
                            }
                            if e.global::<u8>(held.wrapping_add(8)) == 0 {
                                break;
                            }
                            disabled = disabled.wrapping_add(1);
                            if disabled >= size {
                                release(e, held);
                                return;
                            }
                        }
                    }
                } else if disabled > index {
                    disabled = index;
                }
            }
            index += 1;
            if index >= size {
                break;
            }
        }
        if held != 0 {
            release(e, held);
        }
    });
}

// Typed from 00e87200
/// `NiD3DShaderConstantMap::SortByEnabled` (engine map), `thiscall(map)`: both entry arrays.
pub fn sort_by_enabled(e: &mut Engine, this: Ptr<NiD3DShaderConstantMap>) {
    for array in [
        this.at(NiD3DShaderConstantMap::m_aspEntries),
        this.at(NiD3DShaderConstantMap::m_aspPlatformEntries),
    ] {
        typed_call!(
            FAMILY,
            0x00e8_70c0,
            sort_entries_by_enabled(e, this.addr(), array)
        );
    }
}

/// The drawing side's own render state cache (`engine_core::draw_side`): the setters run on either side, each
/// sending what its own copy says the device lacks, and the join leaves main's cache describing the device
/// state the device model's join makes.
#[cfg(test)]
mod side_tests {
    use super::*;
    use engine_core::draw_side::{self, Kind};
    use std::cell::RefCell;

    /// Where the device stand-in's code is.
    const DEVICE_CODE: u32 = 0x7e05_0000;

    thread_local! {
        /// The device calls made: (drawing side, slot, words).
        static CALLS: RefCell<Vec<(bool, u32, Vec<u32>)>> = const { RefCell::new(Vec::new()) };
    }

    fn calls() -> Vec<(bool, u32, Vec<u32>)> {
        CALLS.with(|c| std::mem::take(&mut *c.borrow_mut()))
    }

    /// An engine with a renderer whose state cache has a device that records its calls, the stage-state slot
    /// table (states 0..8 in slots 0..8) and the lock counters mapped. Answers the cache.
    fn engine() -> (Engine, Ptr<NiDX9RenderState>) {
        let mut e = Engine::new();
        for page in [RENDERER, LOCK_COUNTERS, STAGE_SLOT_OF_STATE] {
            e.map(page & !0xfff, 0x1000);
        }
        let cache = e.mem.alloc(CACHE_SIZE);
        let renderer = e.mem.alloc(0x900);
        e.mem.set_u32(renderer + 0x8b8, cache);
        e.mem.set_u32(RENDERER, renderer);
        let vtable = e.mem.alloc(0x200);
        for slot in [
            DEVICE_SET_RENDER_STATE,
            DEVICE_SET_STAGE_STATE,
            DEVICE_SET_TEXTURE,
        ] {
            e.mem.set_u32(vtable + slot, DEVICE_CODE + slot);
            e.register(DEVICE_CODE + slot, |_, a| {
                let drawing = draw_side::is_open_on_drawing_side();
                CALLS.with(|c| c.borrow_mut().push((drawing, 0, a.to_vec())));
                engine_core::abi::Ret::default()
            });
        }
        let device = e.mem.alloc(0x10);
        e.mem.set_u32(device, vtable);
        // The cache's own vtable: its SetRenderState (slot 0x68), reached by address when the family is not
        // direct, is the typed one.
        let cache_vtable = e.mem.alloc(0x100);
        e.mem
            .set_u32(cache_vtable + SLOT_SET_RENDER_STATE, CACHE_SET_RENDER_STATE);
        e.mem.set_u32(cache, cache_vtable);
        e.register(CACHE_SET_RENDER_STATE, |e, a| {
            set_render_state(e, Ptr::new(a[0]), a[1], a[2], a[3], low8(a[4]));
            engine_core::abi::Ret::default()
        });
        e.mem
            .set_u32(cache + NiDX9RenderState::m_pkD3DDevice9.off, device);
        for state in 0..8u16 {
            e.mem
                .set_u16(STAGE_SLOT_OF_STATE + 2 * u32::from(state), state);
        }
        calls();
        (e, Ptr::new(cache))
    }

    fn render_state_of(e: &Engine, cache: Ptr<NiDX9RenderState>, state: u32) -> u32 {
        e.mem.u32(elem_addr(
            cache,
            NiDX9RenderState::auiRenderStates,
            state,
            8,
        ))
    }

    /// Main changes state 7 during the cut; the drawing side changes 7 and 9 and a texture. The drawing side
    /// sends what its own copy lacks and leaves guest memory alone; the join keeps main's 7 (main sent it last,
    /// as the device model's join keeps main's) and takes the drawing side's 9 and texture, so main's next
    /// setters skip exactly what the merged device holds.
    #[test]
    fn each_side_sends_from_its_own_cache_and_the_join_merges_them() {
        let (mut e, cache) = engine();
        set_render_state(&mut e, cache, 7, 1, 0, 0);
        assert_eq!(calls().len(), 1);
        let mut ranges = side_ranges(&e);
        ranges.extend(crate::render::stage::side_ranges());
        assert_eq!(ranges[0], (cache.addr(), CACHE_SIZE, Kind::Device));
        // The stage shadow page is not mapped here; the cache and the counters are the side.
        ranges.truncate(2);
        let side = draw_side::open(&e, &ranges).expect("the side opens");
        set_render_state(&mut e, cache, 7, 2, 0, 0);
        assert_eq!(calls(), [(false, 0, vec![device(&e, cache), 7, 2])]);
        let ((), side) = draw_side::as_drawing_side(side, || {
            set_render_state(&mut e, cache, 7, 3, 0, 0);
            set_render_state(&mut e, cache, 9, 5, 0, 0);
            set_render_state(&mut e, cache, 9, 5, 0, 0);
            set_texture(&mut e, cache, 2, 0x4440);
            set_stage_state(&mut e, cache, 0, 3, 8, 0);
        });
        let dev = device(&e, cache);
        assert_eq!(
            calls(),
            [
                (true, 0, vec![dev, 7, 3]),
                (true, 0, vec![dev, 9, 5]),
                (true, 0, vec![dev, 2, 0x4440]),
                (true, 0, vec![dev, 0, 3, 8]),
            ],
            "the drawing side sends from its copy (state 7 was 1 there), once each"
        );
        assert_eq!(render_state_of(&e, cache, 9), 0, "guest memory is main's");
        let report = draw_side::join(&mut e, side);
        assert_eq!(
            render_state_of(&e, cache, 7),
            2,
            "main's change stays main's"
        );
        assert_eq!(render_state_of(&e, cache, 9), 5);
        assert_eq!(
            e.mem
                .u32(elem_addr(cache, NiDX9RenderState::apTextures, 2, 4)),
            0x4440
        );
        assert_eq!((report.unrouted, report.unbalanced), (0, 0));
        // The device holds 9 = 5 and texture 2 (the drawing side's): main sends neither again.
        set_render_state(&mut e, cache, 9, 5, 0, 0);
        set_texture(&mut e, cache, 2, 0x4440);
        assert!(calls().is_empty());
        set_render_state(&mut e, cache, 7, 2, 0, 0);
        assert!(calls().is_empty(), "7 is main's 2 on the device");
    }

    /// A lock the drawing side takes and gives back leaves the counters as they were; one it keeps is counted
    /// at the join (main would have read it in the serial order).
    #[test]
    fn the_lock_counters_are_balanced_on_the_drawing_side() {
        let (mut e, cache) = engine();
        let ranges = side_ranges(&e);
        let side = draw_side::open(&e, &ranges).unwrap();
        let ((), side) = draw_side::as_drawing_side(side, || {
            set_z_enable(&mut e, 1, 1);
            set_z_enable(&mut e, 1, 0u32.wrapping_sub(1));
        });
        assert_eq!(draw_side::join(&mut e, side).unbalanced, 0);
        let side = draw_side::open(&e, &ranges).unwrap();
        let ((), side) = draw_side::as_drawing_side(side, || set_z_enable(&mut e, 0, 1));
        assert!(draw_side::join(&mut e, side).unbalanced > 0);
        let _ = cache;
    }

    fn device(e: &Engine, cache: Ptr<NiDX9RenderState>) -> u32 {
        e.mem
            .u32(cache.addr() + NiDX9RenderState::m_pkD3DDevice9.off)
    }
}
