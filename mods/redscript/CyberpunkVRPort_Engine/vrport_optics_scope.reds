// CyberpunkVRPort -- the physical sight's controller, a script class the sight's widget names: it stays compiled
// while the Optics module's wrappers are Rust's in the Rust build (rust/crates/cvr-ffi/src/mechanics/reds_hooks.rs).

public native func VRPhysicalOpticsEnabled() -> Bool;

public class VRPhysicalScopeController extends inkGameController {
  protected cb func OnInitialize() -> Bool {
    let root: ref<inkWidget> = this.GetRootWidget();
    root.SetVisible(true);
    root.SetOpacity(1.00);
  }
}
