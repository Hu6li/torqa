# Autumn (#350, R156)

Autumn recolours the shared broadleaf material from `autumn.leaves` in the palette. Stable
tree-position seeds choose red, orange and gold; every authored LOD shares the material.
Needles and solid palm/banana fronds retain their green. Rust's climate boundary leaves
tropical lowlands evergreen, blending seasonal colour above 1,200 m.

Autumn replaces every meadow flower clump with a grass tuft at the same transform. Flower
box blooms are hidden too; switching back to Summer restores its sparse flower population.

The distant mixed canopy retains its crown relief, with stronger orange/russet colour in
roughly 170 m broadleaf-dominant collections and greener evergreen areas between them.
Position-based integer noise keeps these collections coherent at route-scale coordinates.

Fallen red, orange and gold leaves scatter across forest roads, gathering more densely at
their edges in uneven drifts. Small pointed leaf silhouettes have varied angle, size and
midrib shade; fine detail filters to mean coverage with distance. Rain darkens and smooths
them, and road markings remain visible. This changes appearance, not grip or resistance.

Rust derives a bounded woodland image from the retained broad and detailed course maps,
preserving clearings and water and ignoring road ink over woods. The road shader samples
nearby woodland edges too; open fields remain clear and ridden tunnel footprints are
excluded. Tropical evergreen lowlands retain their climate exclusion. The mask is computed
when the course is drawn, not stored with its world; no download is needed. Switching to
Summer clears all seasonal strengths and restores its own plant population.

Review the Gurten climb and hairpin with `SEASON=Autumn`, comparing Summer at the same
cameras. Include Low's distant canopy and a close view of a broadleaf beside a conifer.
`render-seasons.sh` also captures the same forest/open-road materials and a distant forest
plane; `FOREST_REVIEW=1 SEASON=Summer` provides the baseline. The real-route
`autumn-forest-road` and `autumn-forest-rain` cameras select a road within the retained
woodland mask, keeping the review tied to the data used by the material.
No assets, dependencies, generator change or stored-format change are needed.

Reviewed [foliage](images/seasons/autumn-trees.jpg), [road height](images/seasons/autumn-climb.jpg)
and [overview](images/seasons/autumn-hairpin.jpg), against the same Summer views.
The [Low overview](images/seasons/autumn-low.jpg) retains the autumn canopy with fewer trees.
The controlled canopy comparison uses the same camera in [Summer](images/seasons/summer-canopy.jpg)
and [Autumn](images/seasons/autumn-canopy.jpg). The [woodland road material](images/seasons/autumn-road-leaves.jpg)
shows fallen leaves against the [clear open road](images/seasons/autumn-open-road.jpg) and
[Summer baseline](images/seasons/summer-forest-road.jpg). The
[grass-only meadow](images/seasons/autumn-flowers.jpg), real woodland road in
[clear weather](images/seasons/autumn-forest-road.jpg) and [rain](images/seasons/autumn-forest-rain.jpg)
cover R159 and R162.
