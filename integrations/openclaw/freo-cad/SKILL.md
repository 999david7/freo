---
name: freo-cad
description: Design precise mechanical parts in Freo, a Creo-style parametric CAD app, through the tools of its `freo` MCP server. Builds dimensioned parts from datum planes, sketches, extrudes, revolves, holes, rounds, chamfers, shells and patterns; checks volume and mass; renders previews; saves .pfprt parts and exports STL/3MF for 3D printing or STEP for other CAD. Use for brackets, plates, enclosures, spacers, flanges, mounts, adapters and any part with exact millimetre dimensions or that must be printed, machined or fitted. Prefer Blender for organic, sculpted or purely visual models.
version: 1.0.0
metadata: {"openclaw":{"emoji":"📐","os":["darwin"],"requires":{"config":["mcp.servers.freo"]}}}
---

# Freo CAD

Freo is a parametric, history-based modeller (like PTC Creo). You drive it through the
`freo` MCP server: every call edits one part held in memory, regenerates it and answers
with the part's volume, bounding box and errors. Units are **millimetres**.

Freo vs Blender: use Freo when dimensions matter (fits, holes, wall thickness, print or
machining files, STEP for engineers). Use Blender for organic shapes, scenes and renders.

## 1. Plan before calling tools

Write down, before the first call:

1. **Overall size and orientation.** Y is up. Lay the base on `TOP` (the XZ plane) and
   build upwards.
2. **Feature order**, as a machinist would: base solid → bosses/ribs → cuts → holes →
   patterns → rounds and chamfers last (they depend on edges that exist only at the end).
3. **Driving dimensions as parameters** when the user may change them
   (`set_parameters {"W": 120, "T": 8}`, then `"depth": "T"`).
4. **Project folder**: `<slug>/` inside the deliverables folder (the server's
   `FREO_WORK_DIR`, `/Users/Open-Claw/06_3D_Models`). Use relative paths like
   `bracket-m6/bracket-m6.pfprt`; they resolve there.

## 2. Coordinates you must get right

| Plane | Normal | sketch x → | sketch y → |
|---|---|---|---|
| `TOP` / `OBEN` | +Y | world +X | world **−Z** |
| `FRONT` / `VORNE` | +Z | world +X | world +Y |
| `RIGHT` / `RECHTS` | +X | world −Z | world +Y |

- A planar **face** used as a plane has the same local axes as the datum plane with the
  same normal. `"+Y"` means the outermost face pointing up; `"-Y"` the bottom face.
- Faces that are not outermost: call `list_geometry {"kind": "faces"}` and pass
  `{"face": index}`. Pick by `normal`, `center` and `area`.
- Extrudes grow along the plane normal. A cut (`"remove": true`) sketched on a face goes
  into the material; on a datum plane use `"depth_type": "through_all"` or
  `"symmetric"` if in doubt.
- Holes drill against the plane normal. Place them on the face they enter (`"+Y"`),
  at local `x`, `y`.

## 3. Tool recipes

```text
new_part          {"name": "BRACKET_M6"}
add_extrude       {"plane": "TOP", "shapes": [{"type": "rect", "width": 80, "height": 40}], "depth": 6}
add_extrude       {"plane": "+Y", "shapes": [{"type": "circle", "diameter": 20}], "depth": 12}        # boss
add_extrude       {"plane": "FRONT", "shapes": [...], "depth_type": "through_all", "remove": true}  # slot / cut
add_hole          {"plane": "+Y", "x": 30, "y": 0, "diameter": 6.6, "through_all": true}
add_hole          {"plane": "+Y", "x": -30, "y": 0, "type": "counterbore", "diameter": 6.6, "cb_diameter": 11, "cb_depth": 6.5, "through_all": true}
add_pattern       {"features": ["Bohrung 1"], "direction": "X", "count": 3, "spacing": -30}
add_pattern       {"features": [12], "axis": "Y", "count": 6, "angle": 60}                       # bolt circle
add_revolve       {"plane": "FRONT", "shapes": [{"type": "centerline", "from": [0,-1], "to": [0,1]}, {"type": "polygon", "points": [[5,0],[20,0],[20,8],[5,8]]}]}
add_shell         {"faces": ["+Y"], "thickness": 2}                                              # open-top box
add_round         {"edges": {"direction": "Y"}, "radius": 4}                                     # vertical corners
add_chamfer       {"edges": {"face_dir": "+Y", "closed": false}, "distance": 0.8}                 # top outline
add_datum_plane   {"reference": "TOP", "offset": 25}                                             # then sketch on "DTM1"
```

Shapes (sketch-local mm): `rect` (width, height, cx, cy), `circle` (radius or diameter,
cx, cy), `polygon` (points, closed), `polyline`, `line` (from, to), `arc` (center, start,
end, counter-clockwise), `centerline` (from, to; needed by revolves). A circle inside a
rect makes a hole in the profile.

Edge filters combine: `direction` (X/Y/Z), `face_dir` ("+Y"), `face` (index), `convex`,
`closed` (circular edges), `min_length`. If unsure, `list_geometry {"kind": "edges"}` and
pass indices.

Changing things: `edit_feature {"feature": "Profil 1", "set": {"depth": 10}}`; nested
fields with dots (`"sets.0.radius"`); new section with `"shapes"`; see fields with
`get_feature`. `suppress_features`, `resume_features`, `delete_features` (removes
children too), `undo`.

## 4. Check every step

- Read the `model` block of each answer: `errors` must be empty, and `volume_mm3` and
  `bbox.size` must match your plan. Estimate the expected volume yourself (e.g. a hole
  removes π·r²·depth) and compare.
- A tool error means the change was **rolled back**; the part is still as before. Fix the
  arguments (plane, edge filter, dimension) and call again. Do not pile on features after
  an error.
- `mass_properties {"density_g_cm3": 1.24}` (PLA 1.24, PETG 1.27, ABS 1.04,
  aluminium 2.70, steel 7.85) for mass and centre of gravity.

## 5. Look at it

After the main shape and again at the end:

```text
render_png {"path": "<slug>/renders/<slug>-v1.png", "width": 1024, "height": 768}
```

Look at the image. Check holes go through, rounds are on the intended edges, and nothing
is mirrored or rotated. Iterate until it is right; never deliver a part you have not seen.

## 6. Deliver

```text
save_model    {"path": "<slug>/<slug>.pfprt"}           # editable Freo part (opens in Freo.app)
export_model  {"path": "<slug>/export/<slug>.stl"}      # 3D printing (or .3mf)
export_model  {"path": "<slug>/export/<slug>.step"}     # other CAD tools
```

Reply with: a one-line description, key dimensions, volume and mass, the preview image
(send the PNG file from `renders/`), and the exact absolute paths the tools returned.
Mention that the `.pfprt` opens in Freo for further editing.

## Limits

One part per server session (no assemblies or drawings through MCP yet). Calling
`new_part` or `open_model` discards unsaved work, so `save_model` first. No sweeps, blends,
ribs or threads via dedicated tools; `add_feature_json` takes any feature definition (copy
the format from `get_feature`).
