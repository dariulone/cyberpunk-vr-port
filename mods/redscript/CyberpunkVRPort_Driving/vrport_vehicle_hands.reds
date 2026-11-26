// Uses the native door requests also used by Enhanced Vehicle System
// (hgyi56, https://github.com/hgyi56/enhanced-vehicle-system).
// Ownership is checked on this instance at both acquisition and execution.
@addMethod(VehicleObject)
public final func VRPortCanTouchPanel(part: Int32) -> Bool {
  let door: EVehicleDoor;
  let ps: ref<VehicleComponentPS>;
  let interaction: VehicleDoorInteractionState;
  if !this.IsPlayerVehicle() || part < 0 || part >= 6 {
    return false;
  };
  ps = this.GetVehiclePS();
  if !IsDefined(ps) { return false; };
  door = IntEnum<EVehicleDoor>(part);
  interaction = ps.GetDoorInteractionState(door);
  return NotEquals(ps.GetDoorState(door), VehicleDoorState.Detached)
    && NotEquals(interaction, VehicleDoorInteractionState.QuestLocked)
    && NotEquals(interaction, VehicleDoorInteractionState.Reserved);
}

@addMethod(VehicleObject)
public final func VRPortTogglePanel(part: Int32) -> Bool {
  let slot: MountingSlotId;
  let door: EVehicleDoor;
  if !this.VRPortCanTouchPanel(part) { return false; };
  door = IntEnum<EVehicleDoor>(part);
  slot.id = EnumValueToName(n"EVehicleDoor", Cast<Int64>(part));
  if Equals(this.GetVehiclePS().GetDoorState(door), VehicleDoorState.Closed) {
    return VehicleComponent.OpenDoor(this, slot, 0.00);
  };
  return VehicleComponent.CloseDoor(this, slot);
}
