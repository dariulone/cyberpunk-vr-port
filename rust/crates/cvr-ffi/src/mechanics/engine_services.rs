//! The engine's own objects of the port's classes (formerly CyberpunkVRPort_Engine's vrport_lobby_exposure.reds and
//! vrport_optics_scope.reds), made by Rust (script_classes.rs):
//!
//! - VRPortLobbyExposure, a ScriptableService (made with the game's services, its OnLoad called by them): the
//!   callback system's Resource/PostLoad of the environment area parameters and definitions hands every one to
//!   VRLobbyExposureCollect (engine_objects.rs: the lobby's exposure compensation over each setting as it loads), and
//!   Session/Ready puts them back (VRLobbyExposureSet(0)).
//! - VRPhysicalScopeController, the ink controller the physical scope's widget library names: its root shown at full
//!   opacity on its initialization.
use super::lobby_rtti::{self as rt, A};
use super::melee::n;
use crate::script_call::Value;
use crate::script_classes::{ClassDef, Method, CB};
use crate::script_hooks::{Call, Sig};

fn native<T: crate::script_call::Ret>(name: &str, values: &[Value]) -> Option<T> {
    super::belt::native::<T>(name, values)
}

/// OnLoad: the post-load callbacks of the two resource types and the session's ready.
fn lobby_load(call: &mut Call) {
    let this = call.this();
    let callbacks = rt::static_obj("GameInstance", "GetCallbackSystem", &[]);
    if callbacks.is_null() {
        cvr_host::log!("[lobby] exposure service: no callback system\n");
        call.skip();
        return;
    }
    for (function, resource) in [("OnAreaParameters", "worldEnvironmentAreaParameters"), ("OnEnvironment", "worldEnvironmentDefinition")] {
        let handler = rt::obj(&callbacks, "RegisterCallback", &[n("Resource/PostLoad"), A::H(&this), n(function)]);
        let target = rt::static_obj("ResourceTarget", "Type", &[n(resource)]);
        if !handler.is_null() && !target.is_null() {
            rt::call(&handler, "AddTarget", &[A::H(&target)]);
        }
    }
    rt::call(&callbacks, "RegisterCallback", &[n("Session/Ready"), A::H(&this), n("OnSessionReady")]);
    call.skip();
}
/// OnAreaParameters / OnEnvironment: the loaded resource's exposure settings collected.
fn lobby_resource(call: &mut Call) {
    let resource = rt::obj(&call.arg_handle(0), "GetResource", &[]);
    native::<i32>("VRLobbyExposureCollect", &[Value::Handle(&resource)]);
    call.skip();
}
fn lobby_session_ready(call: &mut Call) {
    native::<()>("VRLobbyExposureSet", &[Value::Float(0.0)]);
    call.skip();
}

/// VRPhysicalScopeController.OnInitialize: the scope's root shown.
fn scope_initialize(call: &mut Call) {
    let root = rt::obj(&call.this(), "GetRootWidget", &[]);
    if !root.is_null() {
        rt::call(&root, "SetVisible", &[A::B(true)]);
        rt::call(&root, "SetOpacity", &[A::N(1.0)]);
    }
    call.set_result_bool(true);
}

const NONE: Sig = Sig::Of(&[], "");
// Codeware's native event classes have no "game" prefix (final.redscripts names them ResourceEvent and
// GameSessionEvent): a parameter of a class that does not exist is never handed an event, the callback system does not
// call the function and the lobby's exposure was never collected (the lobby half a stop darker than the C++ build's).
const RESOURCE_EVENT: Sig = Sig::Of(&[("event", "handle:ResourceEvent")], "");
const SESSION_EVENT: Sig = Sig::Of(&[("event", "handle:GameSessionEvent")], "");
pub(crate) const CLASSES: [ClassDef; 2] = [
    ClassDef {
        name: "VRPortLobbyExposure",
        parent: "gameScriptableService",
        methods: &[
            Method { name: "OnLoad", sig: NONE, flags: CB, hook: lobby_load },
            Method { name: "OnAreaParameters", sig: RESOURCE_EVENT, flags: CB, hook: lobby_resource },
            Method { name: "OnEnvironment", sig: RESOURCE_EVENT, flags: CB, hook: lobby_resource },
            Method { name: "OnSessionReady", sig: SESSION_EVENT, flags: CB, hook: lobby_session_ready },
        ],
    },
    ClassDef {
        name: "VRPhysicalScopeController",
        parent: "gameuiWidgetGameController",
        methods: &[Method { name: "OnInitialize", sig: Sig::Of(&[], "Bool"), flags: CB, hook: scope_initialize }],
    },
];
