# Per-instance runtime mesh cut — Cyberpunk 2077 2.5

The cut uses an independent load of the original mesh, not a write to the shared resource or a replacement static body. Only the private depot ID redirects to the original archive payload. Before that copy uploads, clip the touched triangles and interpolate stream attributes, including skin indices/weights. The game thread rechecks the weak component owner and its previous resource before requesting a reload. Slots are bounded (eight resident/in-flight copies, twelve accepted segments per component); no edit is committed to disk or to a save.

| Live run | Observation |
|---|---|
| 18648 | Two NPCs shared body asset 742673908825229339. Different component render proxies; the post-upload CPU buffers were already empty. |
| 20792 | A private resource load had a different CMesh and CRenderMesh. The post-load hook saw 959284 source bytes before upload. |
| 19580 | Direct geometry fixture: the target body changed from 27238 vertices / 137802 indices to 27583 / 138087. The control retained the original resource and the original counts. This was a prescribed cut, not XR input. |
| 18556 | A gameplay receipt and accepted surface marks did **not** by themselves prove a mesh intersection. The bounds-checked projection missed. |
| 28036 | A real Belly hit, followed by fixture blade segments via the surface-trail API. Intersecting the two finite segments with the geometry produced 14 clipped body faces and 44 new vertices; a second mesh had 2 clipped faces. Two ribbon samples were accepted, the serial and HP stayed unchanged. The fixture supplied the subsequent segments, not a headset. |
| 21848 | Final target/control run through the trail API. The target's rendered body changed **27238 → 27280 vertices, 137802 → 137910 indices**. The same body mesh on the control remained at 27238 / 137802. Both accepted trail samples used receipt 1; HP stayed 561.2594. The final body had 14 clipped faces, not two separately verified geometric cuts. |

The channel removes a narrow finite strip from existing faces. This is not a capped solid-body split, cloth simulation, or a second damage event. Bone-local tests approximate a blended pose; unsupported stream layouts, morph-only meshes and ambiguous intersections are not modified. The test of D6 on a pair of solver bodies and the XR continuation limitation are documented separately in `blade-overlap.md`.

## Trail during target motion

In game PID 24860 the fixture submitted two trail segments after a native Belly contact. It then sampled the same three enabled ribbon meshes by entity ID before and during puppet movement. Their world positions changed by 0.5483, 0.5693 and 0.6156 m. Their positions in the animated Belly slot changed by 0.0215, 0.0156 and 0.0736 mm respectively. HP stayed at 561.73615 and the receipt serial stayed at one during trail generation. The simulator supplied the subsequent blade segments.

The private body mesh prepared 14 clipped faces and 42 added vertices. A later upload included both queued segments in its history and produced the same counts. This run verifies two rendered trail segments and one independently measured topology change. The pool keeps a spare resource slot for updating a resident mesh, and collects new segments while the prior upload is in progress.

PID 18604 measured the middle of each sweep as well as its endpoints. The first segment produced 14 clipped faces and 42 added vertices on the body. Replaying both segments on the private resource produced 54 clipped faces and 124 added vertices. The target rendered 27362 vertices and 138078 indices. The control rendered the original 27238 vertices and 137802 indices. A missed middle sample closes the interval between disjoint surfaces. A stroke entering or leaving the mesh can affect its confirmed half.
