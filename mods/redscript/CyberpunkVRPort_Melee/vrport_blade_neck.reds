@if(ModuleExists("Codeware"))
public class VRBladeNeckShapes extends ScriptableService {
  private cb func OnLoad() {
    let callbacks = GameInstance.GetCallbackSystem();
    callbacks.RegisterCallback(n"Resource/PostLoad",this,n"OnShapes")
      .AddTarget(ResourceTarget.Path(r"base\\characters\\base_entities\\npc_base\\npc_base.hitrepresentation"))
      .AddTarget(ResourceTarget.Path(r"base\\characters\\base_entities\\woman_base\\woman_base.hitrepresentation"));
    callbacks.RegisterCallback(n"Entity/Initialize",this,n"OnCharacter").AddTarget(EntityTarget.Type(n"NPCPuppet"));
  }

  private cb func OnShapes(event: ref<ResourceEvent>) {
    let resource = event.GetResource() as HitRepresentationResource;
    let part: HitShapeContainer;
    let sphere: ref<HitShape_Sphere>;
    let data: ref<HitShapeUserDataBase>;
    let item: HitShapeContainer;
    let found: Bool;
    if !IsDefined(resource) { return; };
    for item in resource.representations {
      if Equals(item.name,n"Neck") { return; };
      if Equals(item.name,n"Head") && Equals(item.slotName,n"Head") {
        data = item.userData as HitShapeUserDataBase;
        if IsDefined(data) && Equals(data.GetShapeType(),EHitShapeType.Flesh) && !HitShapeUserDataBase.IsProtectionLayer(data) {
          part = item; found = true;
        };
      };
    };
    if !found { return; };
    sphere = new HitShape_Sphere();
    sphere.radius = 0.045; sphere.rotation = Quaternion(0,0,0,1);
    let transform = Matrix.Identity(); transform.W = Vector4(-0.070,0.0135,0.0,1.0);
    sphere.localTransform = transform; sphere.translation = Vector3(-0.070,0.0135,0.0);
    part.name = n"Neck"; part.slotName = n"Head"; part.shape = sphere;
    ArrayPush(resource.representations,part);
  }
  private cb func OnCharacter(event: ref<EntityLifecycleEvent>) {
    let npc = event.GetEntity() as NPCPuppet;
    let comp: ref<HitRepresentationComponent>;
    let tree: gameHitShapeBVH;
    if !IsDefined(npc) { return; };
    comp = npc.FindComponentByName(n"HitRepresentation") as HitRepresentationComponent;
    if !IsDefined(comp) || !comp.useResourceData { return; };
    let path = ResourceAsyncRef.GetPath(comp.resource);
    if NotEquals(path,r"base\\characters\\base_entities\\npc_base\\npc_base.hitrepresentation")
      && NotEquals(path,r"base\\characters\\base_entities\\woman_base\\woman_base.hitrepresentation") { return; };
    tree = comp.bvhRoot;
    if this.IncludeNeck(tree) { comp.bvhRoot = tree; npc.vrBladeNeckConfigured = true; };
  }
  private final func IncludeNeck(out tree: gameHitShapeBVH) -> Bool {
    let index = 0;
    let child: gameHitShapeBVH;
    if ArrayContains(tree.childrenShapeNames,n"Head") {
      if !ArrayContains(tree.childrenShapeNames,n"Neck") { ArrayPush(tree.childrenShapeNames,n"Neck"); };
      return true;
    };
    while index < ArraySize(tree.childrenNodes) {
      child = tree.childrenNodes[index];
      if this.IncludeNeck(child) { tree.childrenNodes[index] = child; return true; };
      index += 1;
    };
    return false;
  }
}
