module CyberpunkVRPort.WeaponThreat

// Game 2.5 ReactionManagerComponent.OnCrowdReaction. The native ADS/visible-target
// guard is the only changed branch: a live VR barrel lease satisfies it too.
// All fear phases, workspot reactions, vehicle behavior and police calls stay native.
// Explicit 0.00 duration preserves the omitted optional float in native look-at calls.
@replaceMethod(ReactionManagerComponent)
protected cb func OnCrowdReaction(reactionDelayEvent: ref<DelayedCrowdReactionEvent>) -> Bool {
  let blackboard: ref<IBlackboard>;
  let blackboardSystem: ref<BlackboardSystem>;
  let broadcaster: ref<StimBroadcasterComponent>;
  let crowdMemberComponent: ref<CrowdMemberComponent>;
  let exitFearInVehicle: ref<DeescalateFearInVehicle>;
  let exitWorkspot: ref<ExitWorkspotSequenceEvent>;
  let fearPhase: Int32;
  let noAnswer: EntityID;
  let game: GameInstance;
  let pointResults: NavigationFindPointResult;
  let reactionSystem: ref<ReactionSystem>;
  let stimType: gamedataStimType;
  let workspotSystem: ref<WorkspotGameSystem>;
  let owner: ref<GameObject> = this.GetOwner();
  let stimData: stimInvestigateData = reactionDelayEvent.stimEvent.stimInvestigateData;
  if !ScriptedPuppet.IsActive(owner) {
    return false;
  };
  stimType = reactionDelayEvent.stimEvent.GetStimType();
  if this.m_beignHijacked && NotEquals(stimType, gamedataStimType.HijackVehicle) {
    return false;
  };
  game = owner.GetGame();
  reactionSystem = GameInstance.GetReactionSystem(game);
  crowdMemberComponent = this.GetOwnerPuppet().GetCrowdMemberComponent();
  crowdMemberComponent.OnCrowdReaction(stimType);
  if Equals(stimType, gamedataStimType.AimingAt) && reactionDelayEvent.vehicleFearPhase != 3
    && !(owner as NPCPuppet).VRPortIsWeaponAimedAt() {
    blackboardSystem = GameInstance.GetBlackboardSystem(game);
    blackboard = blackboardSystem.GetLocalInstanced(this.GetPlayerSystem().GetLocalPlayerMainGameObject().GetEntityID(), GetAllBlackboardDefs().PlayerStateMachine);
    if blackboard.GetInt(GetAllBlackboardDefs().PlayerStateMachine.UpperBody) == 6 {
      blackboard = GameInstance.GetBlackboardSystem(game).Get(GetAllBlackboardDefs().UI_TargetingInfo);
      if blackboard.GetEntityID(GetAllBlackboardDefs().UI_TargetingInfo.CurrentVisibleTarget) != owner.GetEntityID() && NotEquals(this.m_reactionPreset.Type(), gamedataReactionPresetType.InVehicle_Civilian) {
        this.DeactiveLookAt();
        this.m_desiredFearPhase = -1;
        return IsDefined(null);
      };
    } else {
      this.DeactiveLookAt();
      this.m_desiredFearPhase = -1;
      return IsDefined(null);
    };
  };
  GameInstance.GetDelaySystem(game).CancelDelay(this.m_callingPoliceID);
  fearPhase = this.GetFearReactionPhase(reactionDelayEvent.stimEvent);
  if reactionDelayEvent.vehicleFearPhase > 0 {
    fearPhase = reactionDelayEvent.vehicleFearPhase;
  };
  workspotSystem = GameInstance.GetWorkspotSystem(game);
  crowdMemberComponent.SetThreatLastKnownPosition(this.GetPlayerSystem().GetLocalPlayerMainGameObject().GetWorldPosition());
  crowdMemberComponent.AllowWorkspotsUsage(false);
  NPCPuppet.ChangeHighLevelState(owner, gamedataNPCHighLevelState.Fear);
  if !VehicleComponent.IsMountedToVehicle(game, owner) || Equals(stimType, gamedataStimType.HijackVehicle) {
    reactionSystem.MarkDespawnCandidate(owner.GetEntityID());
  };
  this.DeactiveLookAt();
  this.ResetFacial(0.00);
  this.TriggerFearFacial(fearPhase);
  if this.m_previousFearPhase != 2 {
    AnimationControllerComponent.SetAnimWrapperWeightOnOwnerAndItems(owner, this.GetFearAnimWrapper(fearPhase), 1.00);
    if fearPhase != 0 && (!this.m_fearLocomotionWrapper || fearPhase == 3) {
      AnimationControllerComponent.SetAnimWrapperWeightOnOwnerAndItems(owner, this.GetRandomFearLocomotionAnimWrapper(fearPhase, stimType), 1.00);
    };
    this.m_fearLocomotionWrapper = true;
  };
  switch fearPhase {
    case 1:
      this.m_crowdFearStage = gameFearStage.Stressed;
      break;
    case 2:
      this.m_crowdFearStage = gameFearStage.Alarmed;
      break;
    case 3:
      this.m_crowdFearStage = gameFearStage.Panic;
      crowdMemberComponent.SetThreatLastKnownPosition(this.GetPlayerSystem().GetLocalPlayerMainGameObject().GetWorldPosition());
      break;
    default:
      this.m_crowdFearStage = gameFearStage.Relaxed;
  };
  crowdMemberComponent.ChangeFearStage(this.m_crowdFearStage, !stimData.skipInitialAnimation);
  if workspotSystem.IsActorInWorkspot(owner) {
    if reactionSystem.IsInImmovableWorkspot(owner.GetEntityID()) {
      if fearPhase > 1 && workspotSystem.IsReactionAvailable(owner, n"Fear") {
        this.ActivateReactionLookAt(reactionDelayEvent.stimEvent.sourceObject, false, false, 0.00, true);
        workspotSystem.SendReactionSignal(owner, n"Fear");
        this.m_inReactionSequence = true;
        GameObject.PlayVoiceOver(owner, n"fear_beg", n"Scripts:OnDelayedReaction");
      } else {
        this.TriggerFacialLookAtReaction(true, true);
      };
    } else {
      switch fearPhase {
        case 0:
          this.TriggerFacialLookAtReaction(true, true);
          break;
        case 1:
          pointResults = GameInstance.GetAINavigationSystem(game).FindPointInSphereForCharacter(owner.GetWorldPosition(), 0.50, owner);
          if this.IsTargetClose(reactionDelayEvent.stimEvent.sourceObject, 3.00) && this.IsTargetInFront(reactionDelayEvent.stimEvent.sourceObject) && workspotSystem.IsReactionAvailable(owner, n"Fear") && NotEquals(stimType, gamedataStimType.Bump) {
            this.ActivateReactionLookAt(reactionDelayEvent.stimEvent.sourceObject, false, false, 0.00, true);
            workspotSystem.SendReactionSignal(owner, n"Fear");
            this.m_inReactionSequence = true;
            GameObject.PlayVoiceOver(owner, n"fear_beg", n"Scripts:OnDelayedReaction");
            exitWorkspot = new ExitWorkspotSequenceEvent();
            this.m_exitWorkspotSequenceEventId = GameInstance.GetDelaySystem(game).DelayEvent(owner, exitWorkspot, 3.00);
          } else {
            if Equals(pointResults.status, worldNavigationRequestStatus.OK) && workspotSystem.HasExitNodes(owner, true, false, true) {
              this.TriggerReactionBehaviorForCrowd(reactionDelayEvent.stimEvent, gamedataOutput.WalkAway, false);
            } else {
              if workspotSystem.IsReactionAvailable(owner, n"Fear") && NotEquals(stimType, gamedataStimType.Bump) {
                this.ActivateReactionLookAt(reactionDelayEvent.stimEvent.sourceObject, false, false, 0.00, true);
                workspotSystem.SendReactionSignal(owner, n"Fear");
                this.m_inReactionSequence = true;
                GameObject.PlayVoiceOver(owner, n"fear_beg", n"Scripts:OnDelayedReaction");
                exitWorkspot = new ExitWorkspotSequenceEvent();
                this.m_exitWorkspotSequenceEventId = GameInstance.GetDelaySystem(game).DelayEvent(owner, exitWorkspot, 3.00);
              } else {
                this.TriggerFacialLookAtReaction(true, true);
              };
            };
          };
          break;
        case 2:
          if workspotSystem.IsReactionAvailable(owner, n"Fear") {
            this.ActivateReactionLookAt(reactionDelayEvent.stimEvent.sourceObject, false, false, 0.00, true);
            workspotSystem.SendReactionSignal(owner, n"Fear");
            this.m_inReactionSequence = true;
            GameObject.PlayVoiceOver(owner, n"fear_beg", n"Scripts:OnDelayedReaction");
            exitWorkspot = new ExitWorkspotSequenceEvent();
            this.m_exitWorkspotSequenceEventId = GameInstance.GetDelaySystem(game).DelayEvent(owner, exitWorkspot, 3.00);
          } else {
            if VehicleComponent.IsMountedToVehicle(game, owner) {
              this.ActivateReactionLookAt(reactionDelayEvent.stimEvent.sourceObject, false, false, 0.00, true, true);
              GameObject.PlayVoiceOver(owner, n"fear_beg", n"Scripts:OnCrowdReaction", 0.00, noAnswer, true);
              exitFearInVehicle = new DeescalateFearInVehicle();
              this.m_exitFearInVehicleEventId = GameInstance.GetDelaySystem(game).DelayEvent(owner, exitFearInVehicle, 3.00);
            } else {
              if !this.m_inReactionSequence {
                if fearPhase == 3 {
                  this.SetCrowdRunningAwayAnimFeature(stimType);
                  this.TriggerReactionBehaviorForCrowd(reactionDelayEvent.stimEvent, gamedataOutput.Flee, false);
                } else {
                  this.TriggerReactionBehaviorForCrowd(reactionDelayEvent.stimEvent, gamedataOutput.Surrender, false);
                };
              };
            };
          };
          break;
        case 3:
          if workspotSystem.IsWorkspotEnabled(owner) && !workspotSystem.HasExitNodes(owner, true, false, true) && (workspotSystem.IsReactionAvailable(owner, n"Fear") || this.m_inReactionSequence) {
            this.ActivateReactionLookAt(reactionDelayEvent.stimEvent.sourceObject, false, false, 0.00, true);
            workspotSystem.SendReactionSignal(owner, n"Fear");
            this.m_inReactionSequence = true;
            GameObject.PlayVoiceOver(owner, n"fear_beg", n"Scripts:OnDelayedReaction");
            exitWorkspot = new ExitWorkspotSequenceEvent();
            this.m_exitWorkspotSequenceEventId = GameInstance.GetDelaySystem(game).DelayEvent(owner, exitWorkspot, 3.00);
          } else {
            if this.m_previousFearPhase > 1 || workspotSystem.HasExitNodes(owner, true, false) {
              this.SetCrowdRunningAwayAnimFeature(stimType);
              this.TriggerReactionBehaviorForCrowd(reactionDelayEvent.stimEvent, gamedataOutput.Flee, true);
            } else {
              this.SetCrowdRunningAwayAnimFeature(stimType);
              this.TriggerReactionBehaviorForCrowd(reactionDelayEvent.stimEvent, gamedataOutput.Flee, false);
            };
          };
      };
    };
  } else {
    if this.m_inTrafficLane {
      this.GetOwnerPuppet().GetAIControllerComponent().SetBehaviorArgument(n"StimTarget", ToVariant(reactionDelayEvent.stimEvent.sourceObject));
      switch fearPhase {
        case 0:
          this.TriggerFacialLookAtReaction(true, true);
          break;
        case 1:
          this.TriggerReactionBehaviorForCrowd(reactionDelayEvent.stimEvent, gamedataOutput.WalkAway, false, true);
          break;
        case 2:
          crowdMemberComponent.TryStopTrafficMovement();
          this.TriggerReactionBehaviorForCrowd(reactionDelayEvent.stimEvent, gamedataOutput.Surrender, false, true);
          break;
        case 3:
          this.SetCrowdRunningAwayAnimFeature(stimType);
          this.TriggerReactionBehaviorForCrowd(reactionDelayEvent.stimEvent, gamedataOutput.Flee, false, true);
      };
    } else {
      switch fearPhase {
        case 0:
          this.TriggerFacialLookAtReaction(true, true);
          break;
        case 1:
          pointResults = GameInstance.GetAINavigationSystem(game).FindPointInSphereForCharacter(owner.GetWorldPosition(), 0.50, owner);
          if Equals(pointResults.status, worldNavigationRequestStatus.OK) {
            this.TriggerReactionBehaviorForCrowd(reactionDelayEvent.stimEvent, gamedataOutput.WalkAway, false);
          } else {
            this.TriggerFacialLookAtReaction(true, true);
          };
          break;
        case 2:
          this.TriggerReactionBehaviorForCrowd(reactionDelayEvent.stimEvent, gamedataOutput.Surrender, true);
          break;
        case 3:
          if this.m_previousFearPhase > 1 {
            this.SetCrowdRunningAwayAnimFeature(stimType);
            this.TriggerReactionBehaviorForCrowd(reactionDelayEvent.stimEvent, gamedataOutput.Flee, false, true);
          } else {
            this.SetCrowdRunningAwayAnimFeature(stimType);
            this.TriggerReactionBehaviorForCrowd(reactionDelayEvent.stimEvent, gamedataOutput.Flee, false);
          };
      };
    };
  };
  if NotEquals(this.m_reactionPreset.Type(), gamedataReactionPresetType.Child) && reactionDelayEvent.stimEvent.sourceObject == this.GetPlayerSystem().GetLocalPlayerMainGameObject() && !reactionDelayEvent.stimEvent.IsTagInStimuli(n"NoFearSpread") {
    this.SpreadFear(fearPhase);
  } else {
    if Equals(stimType, gamedataStimType.HijackVehicle) {
      broadcaster = reactionDelayEvent.stimEvent.sourceObject.GetStimBroadcasterComponent();
      if IsDefined(broadcaster) {
        broadcaster.TriggerSingleBroadcast(owner, gamedataStimType.CrimeWitness);
      };
    };
  };
  this.m_desiredFearPhase = -1;
}
