# Touch thumb-rest D-pad modifier

Touch the left thumb rest and move the right stick to send normal gamepad D-pad directions. Threshold is strictly greater than 0.5 on each axis. Look is suppressed throughout the touch, including below the direction threshold; pending snap turns and physical XInput look cannot leak through. L3 shifting and an unmodified L3 sprint tap remain unchanged.

The Boolean `thumbrest_touch` OpenXR action is suggested for the Oculus Touch interaction profile, with left/right `thumbrest/touch` paths. Only the left hand activates this modifier. PSVR2 systems emulating Touch are excluded: their capacitive modifier is Triangle, handled separately. No native Pico binding is added; compatibility depends on the runtime's interaction profile and exposed touch signal.

These are OpenXR inputs translated inside the port. There is no new Cyberpunk `IK_*` XML key for thumb-rest touch. Restart after installing to recreate actions/bindings. Input focus loss clears the modifier; the VR menu retains its own input ownership.

Quest 3/VDXR behavior was exercised in the combined RC2 release. This standalone extraction retains its direction/touch behavior without depending on PSVR2 controls or haptics. `tools/thumbrest_input_tests` covers four directions, diagonals, exact threshold, untouched axes, look suppression and Sense-system exclusion.
