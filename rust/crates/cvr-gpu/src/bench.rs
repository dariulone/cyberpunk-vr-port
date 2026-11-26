//! A WARP bench for the passes' compute shaders (the `bench` feature, the tests' only): textures uploaded and read
//! back, descriptor tables, root constants and a dispatch -- what a test needs to run a pass's HLSL (FXC) and its DXIL
//! made from Rust on the same inputs and compare the bytes.
use windows::core::{Interface, PCSTR};
use windows::Win32::Foundation::{CloseHandle, HANDLE};
use windows::Win32::Graphics::Direct3D::Fxc::{D3DCompile, D3DCOMPILE_OPTIMIZATION_LEVEL3};
use windows::Win32::Graphics::Direct3D::{ID3DBlob, D3D_FEATURE_LEVEL_11_0};
use windows::Win32::Graphics::Direct3D12::*;
use windows::Win32::Graphics::Dxgi::Common::*;
use windows::Win32::Graphics::Dxgi::{CreateDXGIFactory2, IDXGIAdapter, IDXGIFactory4, DXGI_CREATE_FACTORY_FLAGS};
use windows::Win32::System::Threading::{CreateEventW, WaitForSingleObject, INFINITE};

/// A texture of the bench: its resource, format, size, bytes per texel, and whether it is a UAV (kept in
/// UNORDERED_ACCESS) or a render target (kept in RENDER_TARGET); the others rest in COMMON.
pub struct Texture {
    pub resource: ID3D12Resource,
    pub format: DXGI_FORMAT,
    pub width: u32,
    pub height: u32,
    pub texel: u32,
    pub uav: bool,
    pub target: bool,
}

/// A root argument: a descriptor table at a heap slot, root constants, or a root CBV.
pub enum Arg<'a> {
    Table(u32, u32),
    Constants(u32, &'a [u32]),
    Cbv(u32, &'a ID3D12Resource),
}

pub struct Bench {
    pub device: ID3D12Device,
    queue: ID3D12CommandQueue,
    allocator: ID3D12CommandAllocator,
    list: ID3D12GraphicsCommandList,
    fence: ID3D12Fence,
    event: HANDLE,
    done: u64,
    heap: ID3D12DescriptorHeap,
    step: u32,
    rtv: ID3D12DescriptorHeap,
    /// One bench at a time in a process: D3D12 gives every caller the adapter's one device, and two graphics benches
    /// building pipelines on WARP's at once crashed (an access violation in parallel tests, none in serial ones).
    _serial: std::sync::MutexGuard<'static, ()>,
}

const SLOTS: u32 = 64;

fn bytes_per_texel(format: DXGI_FORMAT) -> u32 {
    match format {
        DXGI_FORMAT_R8_UINT | DXGI_FORMAT_R8_UNORM => 1,
        DXGI_FORMAT_R16G16_SINT
        | DXGI_FORMAT_R16G16_FLOAT
        | DXGI_FORMAT_R32_FLOAT
        | DXGI_FORMAT_R32_UINT
        | DXGI_FORMAT_R8G8B8A8_UNORM
        | DXGI_FORMAT_R11G11B10_FLOAT => 4,
        DXGI_FORMAT_R32G32_FLOAT | DXGI_FORMAT_R32G32_SINT | DXGI_FORMAT_R16G16B16A16_FLOAT => 8,
        DXGI_FORMAT_R32G32B32A32_FLOAT => 16,
        other => panic!("the bench has no texel size for {other:?}"),
    }
}

impl Bench {
    /// A WARP device, its queue and list, and a shader-visible heap of 64 descriptors.
    pub fn warp() -> Bench {
        static SERIAL: std::sync::Mutex<()> = std::sync::Mutex::new(());
        let serial = SERIAL.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        // SAFETY (whole function): plain object creation on a WARP adapter; every description lives across its call.
        unsafe {
            let factory: IDXGIFactory4 = CreateDXGIFactory2(DXGI_CREATE_FACTORY_FLAGS(0)).unwrap();
            let adapter: IDXGIAdapter = factory.EnumWarpAdapter().unwrap();
            let mut device: Option<ID3D12Device> = None;
            D3D12CreateDevice(&adapter, D3D_FEATURE_LEVEL_11_0, &mut device).unwrap();
            let device = device.unwrap();
            let queue: ID3D12CommandQueue =
                device.CreateCommandQueue(&D3D12_COMMAND_QUEUE_DESC { Type: D3D12_COMMAND_LIST_TYPE_DIRECT, ..Default::default() }).unwrap();
            let allocator: ID3D12CommandAllocator = device.CreateCommandAllocator(D3D12_COMMAND_LIST_TYPE_DIRECT).unwrap();
            let list: ID3D12GraphicsCommandList = device.CreateCommandList(0, D3D12_COMMAND_LIST_TYPE_DIRECT, &allocator, None).unwrap();
            list.Close().unwrap();
            let fence: ID3D12Fence = device.CreateFence(0, D3D12_FENCE_FLAG_NONE).unwrap();
            let event = CreateEventW(None, false, false, None).unwrap();
            let heap: ID3D12DescriptorHeap = device
                .CreateDescriptorHeap(&D3D12_DESCRIPTOR_HEAP_DESC {
                    Type: D3D12_DESCRIPTOR_HEAP_TYPE_CBV_SRV_UAV,
                    NumDescriptors: SLOTS,
                    Flags: D3D12_DESCRIPTOR_HEAP_FLAG_SHADER_VISIBLE,
                    NodeMask: 0,
                })
                .unwrap();
            let step = device.GetDescriptorHandleIncrementSize(D3D12_DESCRIPTOR_HEAP_TYPE_CBV_SRV_UAV);
            let rtv: ID3D12DescriptorHeap = device
                .CreateDescriptorHeap(&D3D12_DESCRIPTOR_HEAP_DESC { Type: D3D12_DESCRIPTOR_HEAP_TYPE_RTV, NumDescriptors: 1, ..Default::default() })
                .unwrap();
            Bench { device, queue, allocator, list, fence, event, done: 0, heap, step, rtv, _serial: serial }
        }
    }

    fn begin(&self) {
        // SAFETY: the bench's own allocator and list, the GPU idle (every submission waited for).
        unsafe {
            self.allocator.Reset().unwrap();
            self.list.Reset(&self.allocator, None).unwrap();
        }
    }
    fn submit(&mut self) {
        // SAFETY: the list recorded by `begin`'s caller; the fence and event the bench's own.
        unsafe {
            self.list.Close().unwrap();
            self.queue.ExecuteCommandLists(&[Some(self.list.cast().unwrap())]);
            self.done += 1;
            self.queue.Signal(&self.fence, self.done).unwrap();
            if self.fence.GetCompletedValue() < self.done {
                self.fence.SetEventOnCompletion(self.done, self.event).unwrap();
                WaitForSingleObject(self.event, INFINITE);
            }
        }
    }
    fn barrier(&self, resource: &ID3D12Resource, before: D3D12_RESOURCE_STATES, after: D3D12_RESOURCE_STATES) {
        let barrier = D3D12_RESOURCE_BARRIER {
            Type: D3D12_RESOURCE_BARRIER_TYPE_TRANSITION,
            Anonymous: D3D12_RESOURCE_BARRIER_0 {
                Transition: core::mem::ManuallyDrop::new(D3D12_RESOURCE_TRANSITION_BARRIER {
                    pResource: core::mem::ManuallyDrop::new(Some(resource.clone())),
                    Subresource: D3D12_RESOURCE_BARRIER_ALL_SUBRESOURCES,
                    StateBefore: before,
                    StateAfter: after,
                }),
            },
            ..Default::default()
        };
        // SAFETY: a transition of a live resource on the recording list.
        unsafe { self.list.ResourceBarrier(&[barrier.clone()]) };
        // the barrier's reference released
        let transition = core::mem::ManuallyDrop::into_inner(unsafe { barrier.Anonymous.Transition });
        drop(core::mem::ManuallyDrop::into_inner(transition.pResource));
    }
    fn resting(t: &Texture) -> D3D12_RESOURCE_STATES {
        if t.uav {
            D3D12_RESOURCE_STATE_UNORDERED_ACCESS
        } else if t.target {
            D3D12_RESOURCE_STATE_RENDER_TARGET
        } else {
            D3D12_RESOURCE_STATE_COMMON
        }
    }

    /// A render target, its bytes zero.
    pub fn target(&mut self, format: DXGI_FORMAT, width: u32, height: u32) -> Texture {
        self.make(format, width, height, false, true)
    }
    /// A texture, its bytes zero.
    pub fn texture(&mut self, format: DXGI_FORMAT, width: u32, height: u32, uav: bool) -> Texture {
        self.make(format, width, height, uav, false)
    }
    fn make(&mut self, format: DXGI_FORMAT, width: u32, height: u32, uav: bool, target: bool) -> Texture {
        let desc = D3D12_RESOURCE_DESC {
            Dimension: D3D12_RESOURCE_DIMENSION_TEXTURE2D,
            Width: width as u64,
            Height: height,
            DepthOrArraySize: 1,
            MipLevels: 1,
            Format: format,
            SampleDesc: DXGI_SAMPLE_DESC { Count: 1, Quality: 0 },
            Flags: if uav {
                D3D12_RESOURCE_FLAG_ALLOW_UNORDERED_ACCESS
            } else if target {
                D3D12_RESOURCE_FLAG_ALLOW_RENDER_TARGET
            } else {
                D3D12_RESOURCE_FLAG_NONE
            },
            ..Default::default()
        };
        let mut resource: Option<ID3D12Resource> = None;
        let state = if uav {
            D3D12_RESOURCE_STATE_UNORDERED_ACCESS
        } else if target {
            D3D12_RESOURCE_STATE_RENDER_TARGET
        } else {
            D3D12_RESOURCE_STATE_COMMON
        };
        let heap = D3D12_HEAP_PROPERTIES { Type: D3D12_HEAP_TYPE_DEFAULT, ..Default::default() };
        // SAFETY: the description and heap properties on the stack.
        unsafe { self.device.CreateCommittedResource(&heap, D3D12_HEAP_FLAG_NONE, &desc, state, None, &mut resource) }.unwrap();
        let t = Texture { resource: resource.unwrap(), format, width, height, texel: bytes_per_texel(format), uav, target };
        self.upload(&t, &vec![0u8; (width * height * t.texel) as usize]);
        t
    }

    fn footprint(&self, t: &Texture) -> (D3D12_PLACED_SUBRESOURCE_FOOTPRINT, u64) {
        let mut layout = D3D12_PLACED_SUBRESOURCE_FOOTPRINT::default();
        let mut total = 0u64;
        // SAFETY: the texture's own description; out-parameters on the stack.
        unsafe {
            let desc = t.resource.GetDesc();
            self.device.GetCopyableFootprints(&desc, 0, 1, 0, Some(&mut layout), None, None, Some(&mut total));
        }
        (layout, total)
    }
    fn staging(&self, heap: D3D12_HEAP_TYPE, size: u64) -> ID3D12Resource {
        let desc = D3D12_RESOURCE_DESC {
            Dimension: D3D12_RESOURCE_DIMENSION_BUFFER,
            Width: size,
            Height: 1,
            DepthOrArraySize: 1,
            MipLevels: 1,
            SampleDesc: DXGI_SAMPLE_DESC { Count: 1, Quality: 0 },
            Layout: D3D12_TEXTURE_LAYOUT_ROW_MAJOR,
            ..Default::default()
        };
        let state = if heap == D3D12_HEAP_TYPE_UPLOAD { D3D12_RESOURCE_STATE_GENERIC_READ } else { D3D12_RESOURCE_STATE_COPY_DEST };
        let mut resource: Option<ID3D12Resource> = None;
        let properties = D3D12_HEAP_PROPERTIES { Type: heap, ..Default::default() };
        // SAFETY: as in `texture`.
        unsafe { self.device.CreateCommittedResource(&properties, D3D12_HEAP_FLAG_NONE, &desc, state, None, &mut resource) }.unwrap();
        resource.unwrap()
    }

    /// The texture's bytes, rows tightly packed (width * texel bytes each).
    pub fn upload(&mut self, t: &Texture, bytes: &[u8]) {
        let row = (t.width * t.texel) as usize;
        assert_eq!(bytes.len(), row * t.height as usize, "the bytes of a {}x{} texture", t.width, t.height);
        let (layout, total) = self.footprint(t);
        let buffer = self.staging(D3D12_HEAP_TYPE_UPLOAD, total);
        // SAFETY: the upload buffer mapped whole; each row written within its pitch.
        unsafe {
            let mut p: *mut core::ffi::c_void = core::ptr::null_mut();
            buffer.Map(0, None, Some(&mut p)).unwrap();
            for y in 0..t.height as usize {
                let at = (p as *mut u8).add(layout.Offset as usize + y * layout.Footprint.RowPitch as usize);
                core::ptr::copy_nonoverlapping(bytes.as_ptr().add(y * row), at, row);
            }
            buffer.Unmap(0, None);
        }
        self.begin();
        self.barrier(&t.resource, Self::resting(t), D3D12_RESOURCE_STATE_COPY_DEST);
        let dst = D3D12_TEXTURE_COPY_LOCATION {
            pResource: core::mem::ManuallyDrop::new(Some(t.resource.clone())),
            Type: D3D12_TEXTURE_COPY_TYPE_SUBRESOURCE_INDEX,
            Anonymous: D3D12_TEXTURE_COPY_LOCATION_0 { SubresourceIndex: 0 },
        };
        let src = D3D12_TEXTURE_COPY_LOCATION {
            pResource: core::mem::ManuallyDrop::new(Some(buffer.clone())),
            Type: D3D12_TEXTURE_COPY_TYPE_PLACED_FOOTPRINT,
            Anonymous: D3D12_TEXTURE_COPY_LOCATION_0 { PlacedFootprint: layout },
        };
        // SAFETY: both locations alive for the call.
        unsafe { self.list.CopyTextureRegion(&dst, 0, 0, 0, &src, None) };
        drop(core::mem::ManuallyDrop::into_inner(dst.pResource));
        drop(core::mem::ManuallyDrop::into_inner(src.pResource));
        self.barrier(&t.resource, D3D12_RESOURCE_STATE_COPY_DEST, Self::resting(t));
        self.submit();
    }

    /// The texture's bytes, rows tightly packed.
    pub fn read(&mut self, t: &Texture) -> Vec<u8> {
        let (layout, total) = self.footprint(t);
        let buffer = self.staging(D3D12_HEAP_TYPE_READBACK, total);
        self.begin();
        self.barrier(&t.resource, Self::resting(t), D3D12_RESOURCE_STATE_COPY_SOURCE);
        let dst = D3D12_TEXTURE_COPY_LOCATION {
            pResource: core::mem::ManuallyDrop::new(Some(buffer.clone())),
            Type: D3D12_TEXTURE_COPY_TYPE_PLACED_FOOTPRINT,
            Anonymous: D3D12_TEXTURE_COPY_LOCATION_0 { PlacedFootprint: layout },
        };
        let src = D3D12_TEXTURE_COPY_LOCATION {
            pResource: core::mem::ManuallyDrop::new(Some(t.resource.clone())),
            Type: D3D12_TEXTURE_COPY_TYPE_SUBRESOURCE_INDEX,
            Anonymous: D3D12_TEXTURE_COPY_LOCATION_0 { SubresourceIndex: 0 },
        };
        // SAFETY: both locations alive for the call.
        unsafe { self.list.CopyTextureRegion(&dst, 0, 0, 0, &src, None) };
        drop(core::mem::ManuallyDrop::into_inner(dst.pResource));
        drop(core::mem::ManuallyDrop::into_inner(src.pResource));
        self.barrier(&t.resource, D3D12_RESOURCE_STATE_COPY_SOURCE, Self::resting(t));
        self.submit();
        let row = (t.width * t.texel) as usize;
        let mut out = vec![0u8; row * t.height as usize];
        // SAFETY: the readback buffer mapped whole after the copy completed; each row read within its pitch.
        unsafe {
            let mut p: *mut core::ffi::c_void = core::ptr::null_mut();
            buffer.Map(0, None, Some(&mut p)).unwrap();
            for y in 0..t.height as usize {
                let at = (p as *const u8).add(layout.Offset as usize + y * layout.Footprint.RowPitch as usize);
                core::ptr::copy_nonoverlapping(at, out.as_mut_ptr().add(y * row), row);
            }
            buffer.Unmap(0, None);
        }
        out
    }

    fn slot(&self, n: u32) -> D3D12_CPU_DESCRIPTOR_HANDLE {
        assert!(n < SLOTS);
        // SAFETY: the bench's own heap.
        let base = unsafe { self.heap.GetCPUDescriptorHandleForHeapStart() };
        D3D12_CPU_DESCRIPTOR_HANDLE { ptr: base.ptr + (n * self.step) as usize }
    }
    /// The heap's slot `n` as the start of a descriptor table.
    pub fn table(&self, n: u32) -> D3D12_GPU_DESCRIPTOR_HANDLE {
        // SAFETY: the bench's own heap.
        let base = unsafe { self.heap.GetGPUDescriptorHandleForHeapStart() };
        D3D12_GPU_DESCRIPTOR_HANDLE { ptr: base.ptr + (n * self.step) as u64 }
    }
    /// An SRV of `t` (its own format, or `None`: a null SRV of `format`) at slot `n`.
    pub fn srv(&self, n: u32, t: Option<&Texture>, format: DXGI_FORMAT) {
        let desc = D3D12_SHADER_RESOURCE_VIEW_DESC {
            Format: t.map_or(format, |t| t.format),
            ViewDimension: D3D12_SRV_DIMENSION_TEXTURE2D,
            Shader4ComponentMapping: D3D12_DEFAULT_SHADER_4_COMPONENT_MAPPING,
            Anonymous: D3D12_SHADER_RESOURCE_VIEW_DESC_0 { Texture2D: D3D12_TEX2D_SRV { MipLevels: 1, ..Default::default() } },
        };
        // SAFETY: a live resource (or none) and the bench's heap.
        unsafe { self.device.CreateShaderResourceView(t.map(|t| &t.resource), Some(&desc), self.slot(n)) };
    }
    /// A typed buffer SRV (`Buffer<...>` of `format`, `count` elements) over `buffer` at slot `n`.
    pub fn buffer_srv(&self, n: u32, buffer: &ID3D12Resource, format: DXGI_FORMAT, count: u32) {
        let desc = D3D12_SHADER_RESOURCE_VIEW_DESC {
            Format: format,
            ViewDimension: D3D12_SRV_DIMENSION_BUFFER,
            Shader4ComponentMapping: D3D12_DEFAULT_SHADER_4_COMPONENT_MAPPING,
            Anonymous: D3D12_SHADER_RESOURCE_VIEW_DESC_0 { Buffer: D3D12_BUFFER_SRV { FirstElement: 0, NumElements: count, ..Default::default() } },
        };
        // SAFETY: a live buffer and the bench's heap.
        unsafe { self.device.CreateShaderResourceView(buffer, Some(&desc), self.slot(n)) };
    }
    /// A UAV of `t` (or a null UAV of `format`) at slot `n`.
    pub fn uav(&self, n: u32, t: Option<&Texture>, format: DXGI_FORMAT) {
        let desc = D3D12_UNORDERED_ACCESS_VIEW_DESC {
            Format: t.map_or(format, |t| t.format),
            ViewDimension: D3D12_UAV_DIMENSION_TEXTURE2D,
            Anonymous: D3D12_UNORDERED_ACCESS_VIEW_DESC_0 { Texture2D: D3D12_TEX2D_UAV::default() },
        };
        // SAFETY: as in `srv`.
        unsafe { self.device.CreateUnorderedAccessView(t.map(|t| &t.resource), None, Some(&desc), self.slot(n)) };
    }

    /// A compute pipeline of `code` under `root`.
    pub fn pipeline(&self, root: &ID3D12RootSignature, code: D3D12_SHADER_BYTECODE) -> ID3D12PipelineState {
        let desc = D3D12_COMPUTE_PIPELINE_STATE_DESC {
            // SAFETY: the root signature borrowed for the call without a reference of its own.
            pRootSignature: unsafe { core::mem::transmute_copy(root) },
            CS: code,
            ..Default::default()
        };
        // SAFETY: the description alive for the call.
        unsafe { self.device.CreateComputePipelineState(&desc) }.unwrap()
    }

    /// One dispatch: `tables` (root parameter, heap slot), `constants` (root parameter, values), `groups`.
    pub fn dispatch(
        &mut self,
        root: &ID3D12RootSignature,
        pso: &ID3D12PipelineState,
        tables: &[(u32, u32)],
        constants: Option<(u32, &[u32])>,
        groups: [u32; 3],
    ) {
        let args = Self::args(tables, constants);
        self.dispatch_with(root, pso, &args, groups);
    }
    fn args<'a>(tables: &[(u32, u32)], constants: Option<(u32, &'a [u32])>) -> Vec<Arg<'a>> {
        let mut args: Vec<Arg> = tables.iter().map(|&(parameter, slot)| Arg::Table(parameter, slot)).collect();
        if let Some((parameter, values)) = constants {
            args.push(Arg::Constants(parameter, values));
        }
        args
    }
    /// One dispatch with its root arguments.
    pub fn dispatch_with(&mut self, root: &ID3D12RootSignature, pso: &ID3D12PipelineState, args: &[Arg], groups: [u32; 3]) {
        self.begin();
        // SAFETY: the bench's list recording; every object alive until `submit` waited for the GPU.
        unsafe {
            self.list.SetDescriptorHeaps(&[Some(self.heap.clone())]);
            self.list.SetComputeRootSignature(root);
            self.list.SetPipelineState(pso);
            for arg in args {
                match *arg {
                    Arg::Table(parameter, slot) => self.list.SetComputeRootDescriptorTable(parameter, self.table(slot)),
                    Arg::Constants(parameter, values) => {
                        self.list.SetComputeRoot32BitConstants(parameter, values.len() as u32, values.as_ptr().cast(), 0)
                    }
                    Arg::Cbv(parameter, buffer) => self.list.SetComputeRootConstantBufferView(parameter, buffer.GetGPUVirtualAddress()),
                }
            }
            self.list.Dispatch(groups[0], groups[1], groups[2]);
        }
        self.submit();
    }

    /// A constant (upload) buffer holding `bytes`, its size rounded up to 256.
    pub fn constants(&self, bytes: &[u8]) -> ID3D12Resource {
        let size = (bytes.len() as u64).div_ceil(256).max(1) * 256;
        let buffer = self.staging(D3D12_HEAP_TYPE_UPLOAD, size);
        // SAFETY: the upload buffer mapped whole; `bytes` within its size.
        unsafe {
            let mut p: *mut core::ffi::c_void = core::ptr::null_mut();
            buffer.Map(0, None, Some(&mut p)).unwrap();
            core::ptr::copy_nonoverlapping(bytes.as_ptr(), p as *mut u8, bytes.len());
            buffer.Unmap(0, None);
        }
        buffer
    }
}

impl Bench {
    /// A graphics pipeline of `vs` and `ps` under `root`, drawing triangles into one target of `format` (no blending,
    /// no depth, no input layout: the vertex shaders make their vertices from SV_VertexID).
    pub fn graphics(&self, root: &ID3D12RootSignature, vs: D3D12_SHADER_BYTECODE, ps: D3D12_SHADER_BYTECODE, format: DXGI_FORMAT) -> ID3D12PipelineState {
        let mut desc = D3D12_GRAPHICS_PIPELINE_STATE_DESC {
            // SAFETY: the root signature borrowed for the call without a reference of its own.
            pRootSignature: unsafe { core::mem::transmute_copy(root) },
            VS: vs,
            PS: ps,
            SampleMask: u32::MAX,
            PrimitiveTopologyType: D3D12_PRIMITIVE_TOPOLOGY_TYPE_TRIANGLE,
            NumRenderTargets: 1,
            SampleDesc: DXGI_SAMPLE_DESC { Count: 1, Quality: 0 },
            ..Default::default()
        };
        desc.RTVFormats[0] = format;
        desc.RasterizerState.FillMode = D3D12_FILL_MODE_SOLID;
        desc.RasterizerState.CullMode = D3D12_CULL_MODE_NONE;
        desc.RasterizerState.DepthClipEnable = true.into();
        desc.BlendState.RenderTarget[0].RenderTargetWriteMask = D3D12_COLOR_WRITE_ENABLE_ALL.0 as u8;
        // SAFETY: the description alive for the call.
        unsafe { self.device.CreateGraphicsPipelineState(&desc) }.unwrap()
    }

    /// `vertices` vertices drawn as triangles into `target` (the whole of it the viewport).
    #[allow(clippy::too_many_arguments)]
    pub fn draw(
        &mut self,
        root: &ID3D12RootSignature,
        pso: &ID3D12PipelineState,
        target: &Texture,
        tables: &[(u32, u32)],
        constants: Option<(u32, &[u32])>,
        vertices: u32,
    ) {
        let args = Self::args(tables, constants);
        self.draw_with(root, pso, target, &args, vertices);
    }
    /// `vertices` vertices drawn as triangles into `target` with the root arguments `args`.
    pub fn draw_with(&mut self, root: &ID3D12RootSignature, pso: &ID3D12PipelineState, target: &Texture, args: &[Arg], vertices: u32) {
        // SAFETY: the bench's RTV heap and a live target.
        let rtv = unsafe {
            let at = self.rtv.GetCPUDescriptorHandleForHeapStart();
            self.device.CreateRenderTargetView(&target.resource, None, at);
            at
        };
        self.begin();
        // SAFETY: the bench's list recording; every object alive until `submit` waited for the GPU.
        unsafe {
            self.list.SetDescriptorHeaps(&[Some(self.heap.clone())]);
            self.list.SetGraphicsRootSignature(root);
            self.list.SetPipelineState(pso);
            for arg in args {
                match *arg {
                    Arg::Table(parameter, slot) => self.list.SetGraphicsRootDescriptorTable(parameter, self.table(slot)),
                    Arg::Constants(parameter, values) => {
                        self.list.SetGraphicsRoot32BitConstants(parameter, values.len() as u32, values.as_ptr().cast(), 0)
                    }
                    Arg::Cbv(parameter, buffer) => self.list.SetGraphicsRootConstantBufferView(parameter, buffer.GetGPUVirtualAddress()),
                }
            }
            self.list.OMSetRenderTargets(1, Some(&rtv), false, None);
            let viewport = D3D12_VIEWPORT { Width: target.width as f32, Height: target.height as f32, MaxDepth: 1.0, ..Default::default() };
            self.list.RSSetViewports(&[viewport]);
            self.list.RSSetScissorRects(&[windows::Win32::Foundation::RECT { left: 0, top: 0, right: target.width as i32, bottom: target.height as i32 }]);
            self.list.IASetPrimitiveTopology(windows::Win32::Graphics::Direct3D::D3D_PRIMITIVE_TOPOLOGY_TRIANGLELIST);
            self.list.DrawInstanced(vertices, 1, 0, 0);
        }
        self.submit();
    }
}

/// One vertex attribute of a stream-output run: semantic (NUL-terminated), index, format, byte offset.
pub struct Attribute {
    pub semantic: &'static core::ffi::CStr,
    pub index: u32,
    pub format: DXGI_FORMAT,
    pub offset: u32,
}
/// One stream-output element: semantic (NUL-terminated), index, first component, component count.
pub struct Captured {
    pub semantic: &'static core::ffi::CStr,
    pub index: u32,
    pub start: u8,
    pub count: u8,
}

impl Bench {
    /// `vs` run over `vertices` (the input layout `layout`, `stride` bytes each), its outputs `captured` written by
    /// stream output (no rasterization) and read back: `out_stride` bytes a vertex.
    #[allow(clippy::too_many_arguments)]
    pub fn stream_out(
        &mut self,
        root: &ID3D12RootSignature,
        vs: D3D12_SHADER_BYTECODE,
        layout: &[Attribute],
        vertices: &[u8],
        stride: u32,
        captured: &[Captured],
        args: &[Arg],
    ) -> Vec<u8> {
        let count = vertices.len() as u32 / stride;
        let elements: Vec<D3D12_INPUT_ELEMENT_DESC> = layout
            .iter()
            .map(|a| D3D12_INPUT_ELEMENT_DESC {
                SemanticName: windows::core::PCSTR(a.semantic.as_ptr().cast()),
                SemanticIndex: a.index,
                Format: a.format,
                InputSlot: 0,
                AlignedByteOffset: a.offset,
                InputSlotClass: D3D12_INPUT_CLASSIFICATION_PER_VERTEX_DATA,
                InstanceDataStepRate: 0,
            })
            .collect();
        let entries: Vec<D3D12_SO_DECLARATION_ENTRY> = captured
            .iter()
            .map(|c| D3D12_SO_DECLARATION_ENTRY {
                Stream: 0,
                SemanticName: windows::core::PCSTR(c.semantic.as_ptr().cast()),
                SemanticIndex: c.index,
                StartComponent: c.start,
                ComponentCount: c.count,
                OutputSlot: 0,
            })
            .collect();
        let out_stride: u32 = captured.iter().map(|c| c.count as u32 * 4).sum();
        let strides = [out_stride];
        let mut desc = D3D12_GRAPHICS_PIPELINE_STATE_DESC {
            // SAFETY: the root signature borrowed for the call without a reference of its own.
            pRootSignature: unsafe { core::mem::transmute_copy(root) },
            VS: vs,
            StreamOutput: D3D12_STREAM_OUTPUT_DESC {
                pSODeclaration: entries.as_ptr(),
                NumEntries: entries.len() as u32,
                pBufferStrides: strides.as_ptr(),
                NumStrides: 1,
                RasterizedStream: D3D12_SO_NO_RASTERIZED_STREAM,
            },
            InputLayout: D3D12_INPUT_LAYOUT_DESC { pInputElementDescs: elements.as_ptr(), NumElements: elements.len() as u32 },
            SampleMask: u32::MAX,
            PrimitiveTopologyType: D3D12_PRIMITIVE_TOPOLOGY_TYPE_POINT,
            SampleDesc: DXGI_SAMPLE_DESC { Count: 1, Quality: 0 },
            ..Default::default()
        };
        desc.RasterizerState.FillMode = D3D12_FILL_MODE_SOLID;
        desc.RasterizerState.CullMode = D3D12_CULL_MODE_NONE;
        desc.RasterizerState.DepthClipEnable = true.into();
        desc.DepthStencilState.DepthFunc = D3D12_COMPARISON_FUNC_ALWAYS;
        // SAFETY: the description and its arrays alive for the call.
        let pso: ID3D12PipelineState = unsafe { self.device.CreateGraphicsPipelineState(&desc) }.unwrap();
        let input = self.constants(vertices);
        // the stream-output buffer, its filled size in the last 16 bytes
        let size = (out_stride * count) as u64;
        let so = {
            let d = D3D12_RESOURCE_DESC {
                Dimension: D3D12_RESOURCE_DIMENSION_BUFFER,
                Width: size + 16,
                Height: 1,
                DepthOrArraySize: 1,
                MipLevels: 1,
                SampleDesc: DXGI_SAMPLE_DESC { Count: 1, Quality: 0 },
                Layout: D3D12_TEXTURE_LAYOUT_ROW_MAJOR,
                ..Default::default()
            };
            let mut r: Option<ID3D12Resource> = None;
            let heap = D3D12_HEAP_PROPERTIES { Type: D3D12_HEAP_TYPE_DEFAULT, ..Default::default() };
            // SAFETY: as in `texture`.
            unsafe { self.device.CreateCommittedResource(&heap, D3D12_HEAP_FLAG_NONE, &d, D3D12_RESOURCE_STATE_STREAM_OUT, None, &mut r) }.unwrap();
            r.unwrap()
        };
        let back = self.staging(D3D12_HEAP_TYPE_READBACK, size + 16);
        self.begin();
        // SAFETY: the bench's list recording; every object alive until `submit` waited for the GPU.
        unsafe {
            self.list.SetGraphicsRootSignature(root);
            self.list.SetPipelineState(&pso);
            for arg in args {
                match *arg {
                    Arg::Table(parameter, slot) => self.list.SetGraphicsRootDescriptorTable(parameter, self.table(slot)),
                    Arg::Constants(parameter, values) => {
                        self.list.SetGraphicsRoot32BitConstants(parameter, values.len() as u32, values.as_ptr().cast(), 0)
                    }
                    Arg::Cbv(parameter, buffer) => self.list.SetGraphicsRootConstantBufferView(parameter, buffer.GetGPUVirtualAddress()),
                }
            }
            let view = D3D12_VERTEX_BUFFER_VIEW { BufferLocation: input.GetGPUVirtualAddress(), SizeInBytes: vertices.len() as u32, StrideInBytes: stride };
            self.list.IASetVertexBuffers(0, Some(&[view]));
            self.list.IASetPrimitiveTopology(windows::Win32::Graphics::Direct3D::D3D_PRIMITIVE_TOPOLOGY_POINTLIST);
            let target = D3D12_STREAM_OUTPUT_BUFFER_VIEW {
                BufferLocation: so.GetGPUVirtualAddress(),
                SizeInBytes: size,
                BufferFilledSizeLocation: so.GetGPUVirtualAddress() + size,
            };
            self.list.SOSetTargets(0, Some(&[target]));
            let viewport = D3D12_VIEWPORT { Width: 64.0, Height: 64.0, MaxDepth: 1.0, ..Default::default() };
            self.list.RSSetViewports(&[viewport]);
            self.list.DrawInstanced(count, 1, 0, 0);
        }
        self.barrier(&so, D3D12_RESOURCE_STATE_STREAM_OUT, D3D12_RESOURCE_STATE_COPY_SOURCE);
        // SAFETY: both buffers alive; the copy within their sizes.
        unsafe { self.list.CopyBufferRegion(&back, 0, &so, 0, size + 16) };
        self.submit();
        let mut out = vec![0u8; (size + 16) as usize];
        // SAFETY: the readback buffer mapped whole after the copy completed.
        unsafe {
            let mut p: *mut core::ffi::c_void = core::ptr::null_mut();
            back.Map(0, None, Some(&mut p)).unwrap();
            core::ptr::copy_nonoverlapping(p as *const u8, out.as_mut_ptr(), out.len());
            back.Unmap(0, None);
        }
        let filled = u64::from_le_bytes(out[size as usize..size as usize + 8].try_into().unwrap());
        assert_eq!(filled, size, "stream output: {filled} of {size} bytes written");
        out.truncate(size as usize);
        out
    }
}

impl Drop for Bench {
    fn drop(&mut self) {
        // SAFETY: the bench's own event, the GPU idle.
        unsafe {
            let _ = CloseHandle(self.event);
        }
    }
}

/// The HLSL `source`'s `entry` compiled by FXC (`target`, /O3 and the extra `flags`), as the passes compile it.
pub fn fxc(source: &str, entry: &core::ffi::CStr, target: &core::ffi::CStr, flags: u32) -> ID3DBlob {
    let (mut code, mut errors): (Option<ID3DBlob>, Option<ID3DBlob>) = (None, None);
    // SAFETY: the source's bytes and NUL-terminated names alive for the call.
    let made = unsafe {
        D3DCompile(
            source.as_ptr().cast(),
            source.len(),
            PCSTR::null(),
            None,
            None,
            PCSTR(entry.as_ptr().cast()),
            PCSTR(target.as_ptr().cast()),
            D3DCOMPILE_OPTIMIZATION_LEVEL3 | flags,
            0,
            &mut code,
            Some(&mut errors),
        )
    };
    if made.is_err() {
        let text = errors.map(|e| unsafe {
            String::from_utf8_lossy(core::slice::from_raw_parts(e.GetBufferPointer() as *const u8, e.GetBufferSize())).into_owned()
        });
        panic!("{entry:?}: FXC failed: {}", text.unwrap_or_default());
    }
    code.unwrap()
}

/// The HLSL `source`'s `entry` compiled to DXIL by the Windows SDK's dxc.exe (CVR_DXC, or the newest SDK's) -- the
/// bench's own helpers in shader model 6 (a vertex shader giving an engine pass's output signature).
pub fn dxc(source: &str, entry: &str, profile: &str) -> Vec<u8> {
    dxc_with(source, entry, profile, &[])
}
/// As `dxc`, with extra arguments (`-Gis`: IEEE-strict, as the builder compiles the shaders DXC compiled before).
pub fn dxc_with(source: &str, entry: &str, profile: &str, extra: &[&str]) -> Vec<u8> {
    let compiler = std::env::var_os("CVR_DXC").map(std::path::PathBuf::from).unwrap_or_else(|| {
        let root = std::path::PathBuf::from(r"C:\Program Files (x86)\Windows Kits\10\bin");
        let mut found: Vec<_> = std::fs::read_dir(&root)
            .into_iter()
            .flatten()
            .flatten()
            .map(|e| e.path().join("x64").join("dxc.exe"))
            .filter(|p| p.is_file())
            .collect();
        found.sort();
        found.pop().expect("dxc.exe: set CVR_DXC or install the Windows SDK")
    });
    // a folder per call: the tests run in parallel in one process
    static CALLS: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
    let call = CALLS.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let dir = std::env::temp_dir().join(format!("cvr-bench-{}-{call}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let input = dir.join(format!("{entry}.hlsl"));
    let output = dir.join(format!("{entry}.dxil"));
    std::fs::write(&input, source).unwrap();
    let o = std::process::Command::new(compiler)
        .args(["-T", profile, "-E", entry, "-O3"])
        .args(extra)
        .arg("-Fo")
        .arg(&output)
        .arg(&input)
        .output()
        .unwrap();
    assert!(o.status.success(), "dxc: {}{}", String::from_utf8_lossy(&o.stdout), String::from_utf8_lossy(&o.stderr));
    let bytes = std::fs::read(&output).unwrap();
    let _ = std::fs::remove_dir_all(&dir);
    bytes
}
/// DXIL bytes held by the caller as a pipeline takes them.
pub fn code(bytes: &[u8]) -> D3D12_SHADER_BYTECODE {
    D3D12_SHADER_BYTECODE { pShaderBytecode: bytes.as_ptr().cast(), BytecodeLength: bytes.len() }
}

/// A blob's bytes as a pipeline takes them.
pub fn bytecode(blob: &ID3DBlob) -> D3D12_SHADER_BYTECODE {
    // SAFETY: the blob's own buffer, alive as long as the blob.
    unsafe { D3D12_SHADER_BYTECODE { pShaderBytecode: blob.GetBufferPointer(), BytecodeLength: blob.GetBufferSize() } }
}
/// Embedded DXIL as a pipeline takes it.
pub fn dxil(bytes: &'static [u8]) -> D3D12_SHADER_BYTECODE {
    D3D12_SHADER_BYTECODE { pShaderBytecode: bytes.as_ptr().cast(), BytecodeLength: bytes.len() }
}

/// A deterministic byte pattern (xorshift) for a test's inputs.
pub fn pattern(seed: u32, len: usize) -> Vec<u8> {
    let mut x = seed.max(1);
    (0..len)
        .map(|_| {
            x ^= x << 13;
            x ^= x >> 17;
            x ^= x << 5;
            (x >> 24) as u8
        })
        .collect()
}
