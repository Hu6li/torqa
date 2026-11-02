# Winter (#350, R157)

Rust derives an approximate winter snow line from absolute route latitude: 4,200 m in
the tropics, 1,800–600 m from 27–40°, then 600–0 m up to 55°. This is a scenery rule,
not local climate data, a weather forecast or historical snow cover. Both hemispheres
use the same selected winter. The Summer line remains 2,400 m outside the tropics.

The ground retains its slope and submerged-bed exclusions. Shared winter uniforms put
snow on upward-facing foliage, rocks and building roofs, leave needles dense, and thin and brown
temperate broadleaves. Tropical lowlands remain evergreen. Grass under snow and flower
heads are hidden. Resistance and physics are unchanged. Street snow is broken by paired
tyre grooves in each traffic lane, with more continuous cover higher above the snow line.
Grass verges and grassy track centres meet the snowy ground. Tunnel interiors stay clear.

Raised, rounded banks follow the retained ridden-road edges outside the carriageway.
Their settled depth starts around 10 cm, increases by 20 cm per kilometre above the snow
line and caps at 70 cm, fading in across the same 120 m climate transition as surface snow.
Their feet follow the drawn shoulder; joining streets, bridges and tunnels remain open.
Runtime batches occupy 480 m cells and disappear beyond 850 m. These disposable overlays
and the tunnel-exposure UV2 are derived from existing geometry and route tags; they add no
stored fields and require no course regeneration. Snowbanks are a scenery approximation,
not measured snow depth or a change to tyre resistance.

The same altitude profile now raises a continuous blanket on natural ground and snow caps
on boulders (R163), with small drift variation. Greater snow load also covers more inclined
ground, foliage, rocks and roofs; vertical cliffs and submerged beds remain exposed. A
runtime clearance image derived from the retained road, railways, streets, tracks and urban
paving keeps the raised ground from burying ways. Its conservative 16 m-or-coarser cells,
padded around every way, leave a transition beside roads rather than claiming surveyed
ploughing boundaries. All terrain chunks and horizon land sample the same world-space mask
and depth field. Summer restores the original vertex positions; riding geometry is unchanged.

Review the Gurten at road height and above, plus the Oberalp pass, and compare Summer
from identical cameras. Include a tropical lowland fixture in the core climate tests.
The shader changes apply to kept worlds and authored tree LODs without generator or
stored-format changes. No downloaded assets or dependencies are introduced.

Reviewed [vegetation and rocks](images/seasons/winter-trees.jpg),
[covered meadow plants](images/seasons/winter-flowers.jpg),
[roof snow](images/seasons/winter-roof.jpg), the [village](images/seasons/winter-village.jpg)
and [climb](images/seasons/winter-climb.jpg), and the same overview on
[Medium](images/seasons/winter-hairpin.jpg) and [Low](images/seasons/winter-low.jpg).
The [road-height banks](images/seasons/winter-road-close.jpg),
[higher banks on the Oberalp](images/seasons/winter-mountain-close.jpg) and
[clear tunnel interior](images/seasons/winter-tunnel.jpg) cover R161.
The same one-metre, 10 cm-banded stake at [700 m](images/seasons/winter-blanket-700.jpg)
and [2,100 m](images/seasons/winter-blanket-2100.jpg) shows the deeper general blanket and
boulder caps for R163.
