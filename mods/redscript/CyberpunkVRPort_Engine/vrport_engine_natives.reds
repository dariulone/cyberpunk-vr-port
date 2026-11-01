// CyberpunkVRPort -- the plugin's natives for what Codeware added to engine classes, declared once for every script
// of the port: an entity's components, the entities the port spawns (src/Natives/SpawnSystem.cpp) and the lobby's
// exposure settings (src/Natives/EngineObjects.cpp). The Lua mods call the same natives as Game.VR*.

native func VREntityComponents(entity: ref<Entity>) -> array<ref<IComponent>>;

native func VRSpawnTemplate(templatePath: String, appearance: CName, position: Vector4, orientation: Quaternion,
                            tags: array<CName>) -> EntityID;
native func VRSpawnDelete(id: EntityID) -> Bool;
native func VRSpawnedEntity(id: EntityID) -> ref<Entity>;

native func VRLobbyExposureCollect(resource: ref<CResource>) -> Int32;
native func VRLobbyExposureSet(stops: Float);
