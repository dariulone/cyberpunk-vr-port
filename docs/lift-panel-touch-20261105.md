# Lift panels out of reach of the touch rule (2026-11-05)

World touch drops a panel further than 15 m from its owner, as a guard against stale or misattributed panels. A lift's
panel rides the car, so its distance from the owner grows with the shaft.

Measured: a panel 15.44 m from its owner at the top of a shaft. Below the limit the bottom floor's button took a finger, and
above it none did.

The change exempts `LiftDevice` from that rule in `modules/world_touch_sources.lua`. Other devices keep it. The panel still has to be
near the hand, as for any touch, so a far panel cannot be pressed from afar.

Tested in game on Index controllers: the touch clicked in both directions on a lift of that height.
