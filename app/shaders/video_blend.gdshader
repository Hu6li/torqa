// Video ride view (R17): blends from the previous video frame to the next one by how far the
// rider is between them, so the video stays smooth at any speed.
shader_type canvas_item;

uniform sampler2D previous : filter_linear;
uniform float blend : hint_range(0.0, 1.0) = 1.0;

void fragment() {
	COLOR = mix(texture(previous, UV), texture(TEXTURE, UV), blend);
}
