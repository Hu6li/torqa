// Ground of a world file with ORQWA's terrain layers (`ORQWA_terrain_layers`, ADR 0042): the
// author's base colour, often an aerial photo, carries the ground's look from afar; near the
// camera up to eight tiling layers add the detail a photo cannot, each weighted by the author's
// masks and tiled in the world's horizontal plane, or on steep ground, cliffs and banks, from
// the side too, so it never stretches. A layer's albedo is divided by its own average colour,
// so it changes texture, not tone. The world's light is its own (R284): no
// seasons, snow or wetness here.
shader_type spatial;
render_mode cull_back;

#include "res://shaders/authored_terrain.gdshaderinc"
