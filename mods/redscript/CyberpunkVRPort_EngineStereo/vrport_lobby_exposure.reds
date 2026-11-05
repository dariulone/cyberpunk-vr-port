// CyberpunkVRPort — THE LOBBY'S EXPOSURE.
//
// The game's auto exposure shows the main menu's place (the VR lobby: mods/cet/CyberpunkVRPort_Lobby,
// tools/vr_lobby_assets) darker than the reference it is built to, and answers any brightening of the place itself by
// exposing it down. So the place is exposed brighter instead, by the environment's own exposure compensation: the
// environments' exposure settings are collected as their resources load (the world's environment definition, the
// weather states' and areas' parameters), and while the lobby is up the lobby (modules/lobby.lua) sets an offset in
// stops on each of them over what it had; 0 puts them back, and so does every game session that starts.
//
// The game's callback system tells this service of the resources; the settings themselves are native classes no
// script declares, and the plugin keeps them (VRLobbyExposure*, src/Natives/EngineObjects.cpp).

public class VRPortLobbyExposure extends ScriptableService {
  private cb func OnLoad() {
    let callbacks = GameInstance.GetCallbackSystem();
    callbacks.RegisterCallback(n"Resource/PostLoad", this, n"OnAreaParameters")
      .AddTarget(ResourceTarget.Type(n"worldEnvironmentAreaParameters"));
    callbacks.RegisterCallback(n"Resource/PostLoad", this, n"OnEnvironment")
      .AddTarget(ResourceTarget.Type(n"worldEnvironmentDefinition"));
    callbacks.RegisterCallback(n"Session/Ready", this, n"OnSessionReady");
  }

  private cb func OnAreaParameters(event: ref<ResourceEvent>) {
    VRLobbyExposureCollect(event.GetResource());
  }

  private cb func OnEnvironment(event: ref<ResourceEvent>) {
    VRLobbyExposureCollect(event.GetResource());
  }

  private cb func OnSessionReady(event: ref<GameSessionEvent>) {
    VRLobbyExposureSet(0.0);
  }
}
