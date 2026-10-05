shader_type spatial;
// Faceted ground (R45, ADR 0011): one flat colour per facet from the land cover — a vertex colour
// from OpenStreetMap in the palette's sRGB, alpha marking the bed under water — lit with the
// facet's own normal, so the terrain reads as big planes like a diorama. Steep facets are rock,
// high ones snow; each facet differs a little in brightness, so even flat land shows its facets.

uniform vec3 rock_color : source_color = vec3(0.765, 0.733, 0.706);
uniform vec3 snow_color : source_color = vec3(0.949, 0.922, 0.851);
uniform float snow_line = 2400.0;
// A facet steeper than this (its normal's vertical part below it, ~42°) is bare rock.
uniform float rock_slope = 0.74;
// How much a facet's brightness may differ from its neighbours'.
uniform float facet_variation : hint_range(0.0, 0.2) = 0.04;

// From the triangle's first vertex: land-cover borders follow facet edges instead of blending
// across them.
varying flat vec4 cover;
varying flat vec3 corner;

float hash(vec3 p) {
	return fract(sin(dot(p, vec3(127.1, 311.7, 74.7))) * 43758.5453);
}

vec3 to_linear(vec3 srgb) {
	return mix(srgb / 12.92, pow((srgb + 0.055) / 1.055, vec3(2.4)), step(0.04045, srgb));
}

void vertex() {
	cover = COLOR;
	corner = (MODEL_MATRIX * vec4(VERTEX, 1.0)).xyz;
}

void fragment() {
	// The facet's normal from how its surface runs across the screen (view space).
	vec3 facet = normalize(cross(dFdy(VERTEX), dFdx(VERTEX)));
	NORMAL = facet;
	vec3 world_facet = (INV_VIEW_MATRIX * vec4(facet, 0.0)).xyz;
	float water = step(0.5, cover.a);
	float rock = step(world_facet.y, rock_slope) * (1.0 - water);
	float snow = step(snow_line, corner.y) * (1.0 - rock) * (1.0 - water);
	vec3 color = to_linear(cover.rgb);
	color = mix(color, rock_color, rock);
	color = mix(color, snow_color, snow);
	// Stable per facet: its corner and its orientation in the world, not the view.
	float shade = hash(floor(corner * 2.0) + floor(world_facet * 23.0));
	ALBEDO = color * (1.0 + (shade - 0.5) * 2.0 * facet_variation);
	ROUGHNESS = 0.95;
	SPECULAR = 0.12;
}
