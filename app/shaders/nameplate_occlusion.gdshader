shader_type canvas_item;

uniform sampler2D visibility_mask : filter_nearest, repeat_disable;
uniform float slot;

void fragment() {
	if (textureLod(visibility_mask, vec2(slot, 0.5), 0.0).r < 0.5) {
		discard;
	}
}
