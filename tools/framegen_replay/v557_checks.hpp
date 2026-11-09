#include "Framegen/CapturedColor.hpp"
#include "Runtimes/CaptureBufferLeases.hpp"
// Use the actual native three-slot reservation policy, not just COM refs.
void V557BorrowChecks(){
 for(bool privateQueue:{false,true}){
  targeted=privateQueue;Gpu gpu(true);Generator generator;cvr::capture::BufferLeases pool;
  constexpr unsigned w=128,h=96;
  ComPtr<ID3D12Resource> ring[3][2];ID3D12Resource* colors[2]{};
  std::shared_ptr<const Inputs> inputs[2];EyePair cameras[2]{};
  for(unsigned e=0;e<2;++e){
   for(unsigned slot=0;slot<3;++slot){
    ring[slot][e]=gpu.Texture(w,h,DXGI_FORMAT_R8G8B8A8_UNORM,D3D12_RESOURCE_STATE_COPY_DEST);
    Hr(captured_color::MarkShared(ring[slot][e].Get()));
    const std::vector<uint32_t> pixels(w*h,e?0xffe02010:0xff1020e0);
    gpu.Upload(ring[slot][e].Get(),pixels.data(),4,captured_color::ReadState(ring[slot][e].Get()));
   }
   auto in=std::make_shared<Inputs>();in->width=in->motionWidth=w;in->height=in->motionHeight=h;
   in->hasDepth=in->hasMotion=true;in->camera.inverted=true;in->camera.nearPlane=.1f;in->camera.farPlane=100;
   in->camera.verticalFov=1.4f;in->camera.aspect=float(w)/h;
   in->depth=gpu.Texture(w,h,DXGI_FORMAT_R32_FLOAT,D3D12_RESOURCE_STATE_COPY_DEST);
   in->motion=gpu.Texture(w,h,DXGI_FORMAT_R32G32_FLOAT,D3D12_RESOURCE_STATE_COPY_DEST);
   std::vector<float> depth(w*h,.25f),motion(w*h*2,0);
   gpu.Upload(in->depth.Get(),depth.data(),4,D3D12_RESOURCE_STATE_NON_PIXEL_SHADER_RESOURCE);
   gpu.Upload(in->motion.Get(),motion.data(),8,D3D12_RESOURCE_STATE_NON_PIXEL_SHADER_RESOURCE);inputs[e]=in;
   for(auto* c:{&cameras[e].previous,&cameras[e].current}){c->fov[0]=-.8f;c->fov[1]=.8f;c->fov[2]=.7f;c->fov[3]=-.7f;}
  }
  struct Owner {
   cvr::capture::BufferLeases* pool{};ID3D12Resource* images[2]{};
   ~Owner(){for(auto* image:images)if(image)pool->Unpin(reinterpret_cast<uintptr_t>(image));}
  };
  Settings s;s.enabled=true;s.overlay=true;s.backend=Backend::NvidiaV557;s.flowScale=50;SetSettings(s);
  bool interpolated=false;uint64_t frame=0;unsigned slot=0;std::weak_ptr<void> previous;
  // Both backend switch directions exercise the persisted read-state tag.
  for(auto backend:{Backend::NvidiaV557,Backend::Nvidia,Backend::FidelityFX,Backend::NvidiaV557}){
   s.backend=backend;SetSettings(s);
   for(unsigned n=0;n<6;++n){
    generator.Poll();slot=(slot+1)%3;
    auto owner=std::make_shared<Owner>();owner->pool=&pool;
    for(unsigned e=0;e<2;++e){colors[e]=ring[slot][e].Get();const auto key=reinterpret_cast<uintptr_t>(colors[e]);
     Check(pool.BeginWrite(key),"three-slot capture pool stalled by retained history");pool.EndWrite(key);
     Check(pool.Pin(key),"capture pin");owner->images[e]=colors[e];}
    auto submit=[&]{return generator.Generate(gpu.device.Get(),gpu.queue.Get(),colors,inputs,cameras,s,22.22,n==0,++frame,
       &interpolated,gpu.fence.Get(),gpu.value,owner);};
    bool ok=submit();gpu.Wait();if(!ok && n==0){ok=submit();gpu.Wait();}
    Check(ok,"borrowed source generation / backend switch");
    Check(interpolated==(n!=0),"borrowed prime versus interpolation");
    std::weak_ptr<void> current=owner;owner.reset();generator.Poll();
    if(backend==Backend::NvidiaV557){
     Check(!current.expired(),"current B must remain pinned for next pair");
     Check(!pool.BeginWrite(reinterpret_cast<uintptr_t>(colors[0])),"producer overwrote retained B");
     Check(previous.expired(),"old A pin not released after last GPU use");
    }
    previous=current;
    for(unsigned e=0;e<2;++e){const auto pixels=gpu.Read(generator.Output(e));
     Check(e?pixels[2]>pixels[0]+100:pixels[0]>pixels[2]+100,"borrowed eye output identity");}
   }
  }
  Check(generator.Reset(),"borrowed reset");gpu.Wait();Check(previous.expired(),"reset releases final source owner");
  for(auto& pair:ring)for(auto& image:pair){const auto key=reinterpret_cast<uintptr_t>(image.Get());
   Check(pool.BeginWrite(key),"pin leaked after reset");pool.EndWrite(key);}
  printf("PASS borrowed V557: %s queue, three-slot recycling, retained B/released A, legacy backend switches\n",privateQueue?"private":"shared");
 }
 // Direct history contract: identity is borrowed, completion controls release,
 // missing owners/duplicate resources are rejected instead of becoming copies.
 Gpu gpu(true);auto source=gpu.Texture(128,96,DXGI_FORMAT_R8G8B8A8_UNORM,D3D12_RESOURCE_STATE_GENERIC_READ);
 ID3D12Resource* src[]{source.Get()};xrfg::D3D12SwapchainHistory history;
 Hr(history.initialize(gpu.device.Get(),gpu.queue.Get(),src,D3D12_RESOURCE_STATE_GENERIC_READ,nullptr,true));gpu.Wait();
 auto owner=std::make_shared<int>(1);std::weak_ptr<void> weak=owner;
 xrfg::D3D12HistoryCaptureTicket ticket{};
 Check(history.capture_borrowed(source.Get(),gpu.fence.Get(),gpu.value,{},&ticket)==E_INVALIDARG,"missing owner rejected");
 Hr(history.capture_borrowed(source.Get(),gpu.fence.Get(),gpu.value,owner,&ticket));Hr(history.commit(ticket));owner.reset();
 xrfg::D3D12HistoryConsumerLease lease{};ComPtr<ID3D12Resource> borrowed;ComPtr<ID3D12Fence> ready;
 Hr(history.acquire_consumer(ticket,&lease,&borrowed,&ready));Check(borrowed.Get()==source.Get(),"history must not allocate/copy source");
 ComPtr<ID3D12Fence> consumer;Hr(gpu.device->CreateFence(0,D3D12_FENCE_FLAG_NONE,IID_PPV_ARGS(&consumer)));
 Hr(history.retire_consumer(lease,consumer.Get(),1));history.release_completed();Check(!weak.expired(),"pending consumer retains owner");
 xrfg::D3D12HistoryCaptureTicket duplicate{};
 Check(FAILED(history.capture_borrowed(source.Get(),gpu.fence.Get(),gpu.value,std::make_shared<int>(2),&duplicate)),"pending source cannot be recaptured");
 Hr(consumer->Signal(1));history.release_completed();Check(weak.expired(),"completed consumer releases owner");
 Hr(history.invalidate());
 printf("PASS borrowed history identity, owner validation and GPU retirement\n");
}
// Prove both eye preparations can finish while the previous display still owns
// both outputs. This would deadlock behind the first eye in the serial adapter.
void V557BatchChecks(){
 for(bool borrowed:{false,true})for(bool timed:{false,true}){
  Gpu gpu(true);
  constexpr unsigned w=128,h=96;
  std::array<std::shared_ptr<xrfg::D3D12SwapchainHistory>,2> histories;
  std::array<xrfg::D3D12FrameSynthesizer,2> synths;
  ComPtr<ID3D12Resource> source[2],nextSource[2],output[2];
  ComPtr<ID3D12Fence> completions[2],consumer;
  Hr(gpu.device->CreateFence(0,D3D12_FENCE_FLAG_NONE,IID_PPV_ARGS(&consumer)));
  struct Unblock {ID3D12Fence* f;~Unblock(){f->Signal(1);}} unblock{consumer.Get()};
  xrfg::D3D12ReprojectionView view;view.pose.orientation.w=1;
  view.fov={-.8f,.8f,.7f,-.7f};view.image_rect={0,0,w,h};view.array_slice=0;
  xrfg::D3D12FrameSynthesisTicket tickets[2];
  for(unsigned e=0;e<2;++e){
   source[e]=gpu.Texture(w,h,DXGI_FORMAT_R8G8B8A8_UNORM,D3D12_RESOURCE_STATE_COPY_DEST);
   const std::vector<uint32_t> pixels(w*h,e?0xffe02010:0xff1020e0);
   gpu.Upload(source[e].Get(),pixels.data(),4,borrowed?D3D12_RESOURCE_STATE_GENERIC_READ:D3D12_RESOURCE_STATE_COPY_SOURCE);
   if(borrowed){nextSource[e]=gpu.Texture(w,h,DXGI_FORMAT_R8G8B8A8_UNORM,D3D12_RESOURCE_STATE_COPY_DEST);
    gpu.Upload(nextSource[e].Get(),pixels.data(),4,D3D12_RESOURCE_STATE_GENERIC_READ);}
   output[e]=gpu.Texture(w,h,DXGI_FORMAT_R8G8B8A8_UNORM,D3D12_RESOURCE_STATE_COPY_SOURCE,D3D12_RESOURCE_FLAG_ALLOW_RENDER_TARGET);
   histories[e]=std::make_shared<xrfg::D3D12SwapchainHistory>();
   ID3D12Resource* src[]{source[e].Get()},*dst[]{output[e].Get()};
   Hr(histories[e]->initialize(gpu.device.Get(),gpu.queue.Get(),src,borrowed?D3D12_RESOURCE_STATE_GENERIC_READ:D3D12_RESOURCE_STATE_COPY_SOURCE,nullptr,borrowed));
   Hr(synths[e].initialize(gpu.device.Get(),gpu.queue.Get(),histories[e],dst,dst,DXGI_FORMAT_R8G8B8A8_UNORM,
      D3D12_RESOURCE_STATE_COPY_SOURCE,xrfg::D3D12OpticalFlowBackend::nvidia,{},timed,true));gpu.Wait();
   xrfg::D3D12HistoryCaptureTicket capture;
   Hr(borrowed?histories[e]->capture_borrowed(source[e].Get(),gpu.fence.Get(),gpu.value,std::make_shared<int>(1),&capture):
      histories[e]->capture_source(source[e].Get(),&capture));Hr(histories[e]->commit(capture));
   Hr(synths[e].submit_prime(capture,{&view,1},0,&tickets[e]));gpu.Wait();
   xrfg::D3D12NvidiaGpuTiming primeTiming{};
   static_cast<void>(synths[e].consume_nvidia_gpu_timing(&primeTiming));
   Hr(synths[e].completion_fence(&completions[e]));
   synths[e].output_consumer(consumer.Get(),1);
  }
  for(unsigned e=0;e<2;++e){
   xrfg::D3D12HistoryCaptureTicket capture;
   Hr(borrowed?histories[e]->capture_borrowed(nextSource[e].Get(),gpu.fence.Get(),gpu.value,std::make_shared<int>(2),&capture):
      histories[e]->capture_source(source[e].Get(),&capture));Hr(histories[e]->commit(capture));
   Check(synths[e].submit_pair(capture,{&view,1},{&view,1},0,0,&tickets[e],std::nullopt,{},true,.5f,std::nullopt,true)==E_INVALIDARG,
      "deferred reconstruction rejects deferred real copy");
   Hr(synths[e].submit_pair(capture,{&view,1},{&view,1},0,0,&tickets[e],std::nullopt,{},false,.5f,std::nullopt,true));
  }
  // There is deliberately no consumer signal yet. Both packs must drain.
  gpu.Wait();
  for(unsigned e=0;e<2;++e){
   Check(completions[e]->GetCompletedValue()<tickets[e].fence_value,"prepare must not signal completed output");
   Hr(synths[e].flush_reconstruction());Hr(synths[e].flush_reconstruction());
  }
  Check(completions[0]->GetCompletedValue()<tickets[0].fence_value &&
        completions[1]->GetCompletedValue()<tickets[1].fence_value,"output waits preserve consumer ownership");
  Hr(consumer->Signal(1));gpu.Wait();
  for(unsigned e=0;e<2;++e){
   Check(completions[e]->GetCompletedValue()>=tickets[e].fence_value,"both completion receipts signaled");
   const auto pixels=gpu.Read(output[e].Get());
   Check(e?pixels[2]>pixels[0]+100:pixels[0]>pixels[2]+100,"batched eyes remain independent");
   xrfg::D3D12NvidiaGpuTiming t{};
   Check(synths[e].consume_nvidia_gpu_timing(&t)==(timed?S_OK:S_FALSE),"deferred timing resolves");
   if(timed)Check(t.gpu_end_qpc>=t.gpu_begin_qpc && t.total_microseconds>0,"valid deferred GPU timing");
  }
  printf("PASS V557 both-eye preparation before consumer wait: %s history, timing %s\n",borrowed?"borrowed":"copied",timed?"on":"off");
 }
}
// GPU integration checks for the production adapter, without a game or XR session.
void V557Checks(){
 V557BorrowChecks();
 V557BatchChecks();
 for(bool privateQueue:{false,true}){
  targeted=privateQueue;Gpu gpu(true);Generator generator;
  Settings s;s.enabled=true;s.overlay=true;s.backend=Backend::NvidiaV557;s.flowScale=50;SetSettings(s);
  constexpr unsigned w=128,h=96;
  ComPtr<ID3D12Resource> colorsOwned[2];ID3D12Resource* colors[2];std::shared_ptr<const Inputs> inputs[2];EyePair cameras[2];
  for(unsigned e=0;e<2;++e){
   colorsOwned[e]=gpu.Texture(w,h,DXGI_FORMAT_R8G8B8A8_UNORM,D3D12_RESOURCE_STATE_COPY_DEST);colors[e]=colorsOwned[e].Get();
   std::vector<uint32_t> pixels(w*h,e?0xffe02010:0xff1020e0);
   for(unsigned y=30;y<60;++y)for(unsigned x=30;x<60;++x)pixels[y*w+x]=0xffa0a0a0;
   gpu.Upload(colors[e],pixels.data(),4,D3D12_RESOURCE_STATE_COPY_SOURCE);
   auto in=std::make_shared<Inputs>();in->width=in->motionWidth=w;in->height=in->motionHeight=h;
   in->hasDepth=in->hasMotion=true;in->camera.inverted=true;in->camera.nearPlane=.1f;in->camera.farPlane=100;
   in->camera.verticalFov=1.4f;in->camera.aspect=float(w)/h;
   in->depth=gpu.Texture(w,h,DXGI_FORMAT_R32_FLOAT,D3D12_RESOURCE_STATE_COPY_DEST);
   in->motion=gpu.Texture(w,h,DXGI_FORMAT_R32G32_FLOAT,D3D12_RESOURCE_STATE_COPY_DEST);
   std::vector<float> depth(w*h,.25f),motion(w*h*2,0);
   gpu.Upload(in->depth.Get(),depth.data(),4,D3D12_RESOURCE_STATE_NON_PIXEL_SHADER_RESOURCE);
   gpu.Upload(in->motion.Get(),motion.data(),8,D3D12_RESOURCE_STATE_NON_PIXEL_SHADER_RESOURCE);inputs[e]=in;
   for(auto* c:{&cameras[e].previous,&cameras[e].current}){c->fov[0]=-.8f;c->fov[1]=.8f;c->fov[2]=.7f;c->fov[3]=-.7f;}
  }
  bool interpolated=false;
  auto generate=[&](bool reset){return generator.Generate(gpu.device.Get(),gpu.queue.Get(),colors,inputs,cameras,s,22.22,reset,1,&interpolated,gpu.fence.Get(),gpu.value);};
  bool primed=generate(true);gpu.Wait();if(!primed){primed=generate(true);gpu.Wait();}
  Check(primed && !interpolated,"V557 prime must not publish synthetic");
  Check(generator.CanPrepare(gpu.device.Get(),gpu.queue.Get(),colors,inputs,s)==privateQueue,"prepare only on targeted private queue");
  Check(generate(false),"V557 pair");gpu.Wait();Check(interpolated && generator.OutputReady(),"paired output receipt");
  for(unsigned e=0;e<2;++e){const auto pixels=gpu.Read(generator.Output(e));
   Check(e?pixels[2]>pixels[0]+100:pixels[0]>pixels[2]+100,"both eyes retain independent colors");}
  Check(generator.Bytes()>w*h*4*8,"reconstruction allocation accounting");
  // A consumer receipt must block Reset/backend replacement and preserve resources.
  ComPtr<ID3D12CommandQueue> display;D3D12_COMMAND_QUEUE_DESC desc{};Hr(gpu.device->CreateCommandQueue(&desc,IID_PPV_ARGS(&display)));
  ComPtr<ID3D12Fence> gate,receipt;Hr(gpu.device->CreateFence(0,D3D12_FENCE_FLAG_NONE,IID_PPV_ARGS(&gate)));
  Hr(gpu.device->CreateFence(0,D3D12_FENCE_FLAG_NONE,IID_PPV_ARGS(&receipt)));
  struct Release {ComPtr<ID3D12Fence> gate;~Release(){if(gate)gate->Signal(1);}} release{gate};
  Hr(display->Wait(gate.Get(),1));Hr(display->Signal(receipt.Get(),1));generator.ConsumerFence(receipt.Get(),1);
  Check(!generator.Reset(),"display still owns output");
  s.backend=Backend::Nvidia;SetSettings(s);Check(!generate(true),"backend switch must wait for display receipt");
  Hr(gate->Signal(1));Hr(gpu.queue->Wait(receipt.Get(),1));gpu.Wait();
  Check(generate(true),"switch to existing NVIDIA backend");gpu.Wait();Check(!interpolated && ActiveBackend()==Backend::Nvidia,"existing backend primes after switch");
  Check(generate(false),"existing backend pair after switch");gpu.Wait();
  s.backend=Backend::NvidiaV557;SetSettings(s);primed=generate(true);gpu.Wait();if(!primed){primed=generate(true);gpu.Wait();}
  Check(primed && !interpolated,"switch back safely primes V557");
  if(privateQueue){
   // Hold capture upstream without blocking Generate's CPU thread. A deferred output
   // must not be joined onto the display queue until requested by Presenter.
   ComPtr<ID3D12Fence> captureGate;Hr(gpu.device->CreateFence(0,D3D12_FENCE_FLAG_NONE,IID_PPV_ARGS(&captureGate)));
   Release captureRelease{captureGate};Hr(gpu.queue->Wait(captureGate.Get(),1));Hr(gpu.queue->Signal(gpu.fence.Get(),++gpu.value));
   CaptureTimingPool pool;auto sample=pool.Next(gpu.device.Get());Check(bool(sample),"capture timing allocation");
   gpu.Begin();sample->Start(gpu.list.Get());sample->End(gpu.list.Get());Hr(gpu.list->Close());
   ID3D12CommandList* lists[]{gpu.list.Get()};sample->Enqueue(gpu.queue.Get());gpu.queue->ExecuteCommandLists(1,lists);
   Hr(gpu.queue->Signal(gpu.fence.Get(),++gpu.value));sample->Submitted(gpu.fence.Get(),gpu.value);
   const auto oldPairs=capturePairs,oldInvalid=captureInvalid,oldQueueSamples=queueSamples;
   flight::enabled=true;
   auto lease=std::make_shared<int>(7);std::weak_ptr<void> weak=lease;
   const auto begin=GetTickCount64();
   Check(generator.Generate(gpu.device.Get(),gpu.queue.Get(),colors,inputs,cameras,s,22.22,false,3,&interpolated,
      gpu.fence.Get(),gpu.value,lease,display.Get(),sample,true),"deferred private pair");
   lease.reset();Check(GetTickCount64()-begin<200,"Generate blocked on capture CPU wait");
   Check(!generator.OutputReady() && !weak.expired() && !generator.Reset(),"pending GPU capture retains ownership");
   Hr(captureGate->Signal(1));Check(generator.JoinOutput(gpu.queue.Get()),"deferred output join");gpu.Wait();generator.Poll();
   Check(generator.OutputReady() && weak.expired(),"receipt completion releases input lease");
   Check(capturePairs==oldPairs+1 && captureInvalid==oldInvalid && queueSamples==oldQueueSamples+1,
      "V557 capture-to-generation correlation restored");
   Check(GetStatistics().generationSpanMs>=GetStatistics().generationMs,"pair span covers active GPU work");
   flight::enabled=false;
  }
  Check(generate(true),"recenter/reset priming");gpu.Wait();Check(!interpolated,"reset never emits stale synthetic");
  Check(generate(false),"generation recovers after reset");gpu.Wait();
  Check(generator.Reset(),"completed V557 reset");gpu.Wait();
  printf("PASS V557 GPU: %s queue, separate eyes, priming/reset, backend switching, consumer protection, capture lease\n",privateQueue?"private":"shared");
 }
 Check(syncFailures==0,"unexpected GPU synchronization failure");
}
