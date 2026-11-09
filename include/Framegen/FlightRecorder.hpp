#pragma once
#include <atomic>
#include <array>
#include <cstdint>
#include <cstddef>
#include <cmath>

namespace cvr::framegen {struct DisplayDeadline;struct SourceRecord;}
namespace cvr::framegen::flight {
enum class Event : unsigned {
    Capture, InputLeft, InputRight, PairGap, PairRejected, WaitingInputs, Paused,
    GeneratorBusy, GenerateFailed, Primed, GeneratedQueued,
    AcceptedReal, AcceptedSynthetic, AcceptedRepeat, SubmitRejected,
    AllocatorBusy, CaptureNoSlot, CaptureTimeout, FallbackRepeat, EmptyEnd,
    XrError, Cycle, Marker, SettingsChange, BeginDiscarded, PacedCycle, PacerUnavailable, PhaseJump, PhaseLater, PhaseEarlier, PacerTimerFailed, PacerNoReading, PacerNonpositive, PacerOutOfRange, RealGpuReady, RealGpuPending, SyntheticGpuReady, SyntheticGpuPending, SyntheticGpuPendingBeforeHold, SynthesisPrivate, SynthesisShared, SynthesisQueueFallback, SynthesisInputFallback, SynthesisSyncFailed, PresentationCopy, PresentationApplicationWait, PresentationRuntimeWait, PresentationSyncFailed, PresentationReady, PresentationPending, PhaseGpuPending, PhaseGpuRecovery, GenerationProbe, GenerationCapturePending, GenerationGuidesPending, GenerationConsumerPending, GenerationCalibrationFailed, GenerationCalibrationInvalid, OverlaySourceReady, OverlaySourcePending, SynthesisConsumerDeferred, SynthesisConsumerLatePending, CaptureProbeQueued, CaptureProbeSkipped, CaptureProbePaired, CaptureProbeInvalid, ProducerGpuReady, ProducerGpuWait, ProducerGpuReleased, ProducerGpuTimeout, ProducerGpuWaitFailed, ProducerGpuDeviceLost, ProducerGpuMissing, DisplayProbeQueued, DisplayProbeSkipped, DisplayProbeInvalid, DisplayProbeRejected, DisplayReal, DisplaySynthetic, DisplayRepeat, DisplayBoundaryFuture, DisplayGpuBeforeHandoff, DisplayGpuAfterHandoff, DisplayHandoffAmbiguous, DisplayGpuEarlyBoundary, DisplayGpuLateBoundary, DisplayBoundaryAmbiguous, DisplayHandoffEarlyTarget, DisplayHandoffLateTarget, DisplayTargetAmbiguous, DisplayRealGpuLate, DisplaySyntheticGpuLate, DisplayRepeatGpuLate, DisplayCpuAndGpuEarly, DisplayCpuOnlyLate, DisplayGpuOnlyLate, DisplayBothLate, DisplayRecordDropped, PrepareAheadAttempt, PrepareAheadQueued, PrepareAheadUsed, PrepareAheadCancelled, PrepareAheadFailed, PrepareAheadReady, PrepareAheadPending, PrepareAheadOverrun, SourceMarkerRecorded, SourceMarkerSkipped, SourceMarkerDuplicate, SourceMarkerReplay, SourceMarkerSignalFailed, SourcePaired, SourceInvalid, SourceRecordDropped, Count
};
inline constexpr const char* Names[]={"capture","input_left","input_right","pair_gap","pair_rejected",
    "waiting_inputs","paused","generator_busy","generate_failed","primed","generated_queued",
    "accepted_real","accepted_synthetic","accepted_repeat","submit_rejected","allocator_busy",
    "capture_no_slot","capture_timeout","fallback_repeat","empty_end","xr_error","cycle","marker","settings_change","begin_discarded","paced_cycle","pacer_unavailable","phase_jump","phase_later","phase_earlier","pacer_timer_failed","pacer_no_reading","pacer_nonpositive","pacer_out_of_range","real_gpu_ready","real_gpu_pending","synthetic_gpu_ready","synthetic_gpu_pending","synthetic_gpu_pending_before_hold","synthesis_private","synthesis_shared","synthesis_queue_fallback","synthesis_input_fallback","synthesis_sync_failed","presentation_copy","presentation_application_wait","presentation_runtime_wait","presentation_sync_failed","presentation_ready","presentation_pending","phase_gpu_pending","phase_gpu_recovery","generation_probe","generation_capture_pending","generation_guides_pending","generation_consumer_pending","generation_calibration_failed","generation_calibration_invalid","overlay_source_ready","overlay_source_pending","synthesis_consumer_deferred","synthesis_consumer_late_pending","capture_probe_queued","capture_probe_skipped","capture_probe_paired","capture_probe_invalid","producer_gpu_ready","producer_gpu_wait","producer_gpu_released","producer_gpu_timeout","producer_gpu_wait_failed","producer_gpu_device_lost","producer_gpu_missing","display_probe_queued","display_probe_skipped","display_probe_invalid","display_probe_rejected","display_real","display_synthetic","display_repeat","display_boundary_future","display_gpu_before_handoff","display_gpu_after_handoff","display_handoff_ambiguous","display_gpu_early_boundary","display_gpu_late_boundary","display_boundary_ambiguous","display_handoff_early_target","display_handoff_late_target","display_target_ambiguous","display_real_gpu_late","display_synthetic_gpu_late","display_repeat_gpu_late","display_cpu_and_gpu_early","display_cpu_only_late","display_gpu_only_late","display_both_late","display_record_dropped","prepare_ahead_attempt","prepare_ahead_queued","prepare_ahead_used","prepare_ahead_cancelled","prepare_ahead_failed","prepare_ahead_ready","prepare_ahead_pending","prepare_ahead_overrun","source_marker_recorded","source_marker_skipped","source_marker_duplicate","source_marker_replay","source_marker_signal_failed","source_paired","source_invalid","source_record_dropped"};
static_assert(std::size(Names)==size_t(Event::Count));
enum class Timing : unsigned { GenerationGpu, GenerationSpan, EndFrame, CaptureWait, ImageAge, WaitFrame, PacingBudget, SubmitHold, SubmitProbe, RealRemaining, SyntheticRemaining, RepeatRemaining, SubmitMargin, PhaseStep, RealHandoffGap, SyntheticHandoffGap, RepeatHandoffGap, RealEnd, SyntheticEnd, RepeatEnd, PrepareBeforeHold, GenerationQueue, GenerationTotal, GenerationQueueInputsReady, GenerationQueueInputsPending, GenerationCalibrationCpu, CaptureQueue, CaptureGpu, CaptureTotal, GenerationBeforeCaptureReady, GenerationAfterCaptureReady, ProducerCpuHold, ProducerGpuHold, DisplayQueue, DisplayProbeWidth, DisplayRemaining, DisplayGpuLeadBoundary, DisplayGpuLagBoundary, DisplayGpuLeadHandoff, DisplayGpuLagHandoff, DisplayHandoffLeadTarget, DisplayHandoffLagTarget, DisplayGpuLeadTarget, DisplayGpuLagTarget, PrepareAheadLead, PrepareAheadCpu, PrepareAheadOverrun, Count };
inline constexpr const char* TimingNames[]={"generation_gpu_ms","generation_span_ms","xr_end_ms","capture_wait_ms","source_age_ms","xr_wait_ms","pacing_budget_ms","submit_hold_ms","submit_probe_ms","real_remaining_ms","synthetic_remaining_ms","repeat_remaining_ms","submit_margin_ms","phase_step_abs_ms","real_handoff_gap_ms","synthetic_handoff_gap_ms","repeat_handoff_gap_ms","real_xr_end_ms","synthetic_xr_end_ms","repeat_xr_end_ms","prepare_before_hold_ms","generation_enqueue_to_start_ms","generation_enqueue_to_end_ms","generation_queue_inputs_ready_ms","generation_queue_inputs_pending_ms","generation_calibration_cpu_ms","capture_enqueue_to_start_ms","capture_gpu_span_ms","capture_enqueue_to_end_ms","generation_before_capture_ready_ms","generation_after_capture_ready_ms","producer_cpu_hold_ms","producer_gpu_hold_ms","display_queue_ms","display_probe_width_ms","display_remaining_ms","display_gpu_lead_boundary_ms","display_gpu_lag_boundary_ms","display_gpu_lead_handoff_ms","display_gpu_lag_handoff_ms","display_handoff_lead_target_ms","display_handoff_lag_target_ms","display_gpu_lead_target_ms","display_gpu_lag_target_ms","prepare_ahead_lead_ms","prepare_ahead_cpu_ms","prepare_ahead_overrun_ms"};
static_assert(std::size(TimingNames)==size_t(Timing::Count));
struct Sample {uint64_t count{},sumUs{},maxUs{};};
struct Accumulator {
    std::atomic<uint64_t> count{},sum{},maximum{};
    void Add(double ms) {
        if(!std::isfinite(ms)||ms<0||ms>60000)return;
        auto us=uint64_t(ms*1000);sum.fetch_add(us,std::memory_order_relaxed);
        count.fetch_add(1,std::memory_order_relaxed);
        auto old=maximum.load(std::memory_order_relaxed);
        while(us>old&&!maximum.compare_exchange_weak(old,us,std::memory_order_relaxed)){}
    }
    // Window aggregates are approximate at a concurrent sample boundary.
    Sample Drain(){return {count.exchange(0),sum.exchange(0),maximum.exchange(0)};}
};
inline std::atomic<bool> enabled{false};
inline bool Enabled(){return enabled.load(std::memory_order_relaxed);}
void Count(Event);
void Source(const SourceRecord&);
void Display(const DisplayDeadline&,double gpuEndMs,double queueMs);
void Time(Timing,double ms);
void Input(unsigned eye,uint32_t width,uint32_t height);
void Output(uint32_t width,uint32_t height);
void Period(double ms);
void Pace(double interval,bool active);
void Error(int64_t result);
void Configure(bool requested);
void Runtime(const char* name,bool steamvr,const char* headset);
void Stop(); // before destroying an XR session; preserves the checkbox preference
void ReleaseConnection(); // only at instance destruction
int State(); // 0 off, 1 waiting for runtime, 2 writing, -1 IO/worker failure, -2 file cap
void Status(char* text,size_t capacity);
}
