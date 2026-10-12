class_name RadarChart
extends Control
## The power profile (R63): one spoke per duration, the rim the rider's best of all, and the
## period's best as a filled shape inside, each corner labelled with its share and value.

const FONT_SIZE: int = 12
## Room around the rim for the labels.
const MARGIN: float = 56.0
const RINGS: Array[float] = [0.25, 0.5, 0.75, 1.0]
const GRID: Color = Color(1, 1, 1, 0.08)
const RIM: Color = Color(1, 1, 1, 0.3)

## Per spoke: `label`, `share` (0–1, negative without data) and `value` (text under the share).
var _spokes: Array[Dictionary] = []


## One spoke per entry of `labels` (clockwise from the top), with the period's `shares` of the
## rider's best (negative where there is none) and `values` written beside them.
func set_spokes(
	labels: PackedStringArray, shares: PackedFloat64Array, values: PackedStringArray
) -> void:
	_spokes.clear()
	for i: int in range(labels.size()):
		var share: float = shares[i] if i < shares.size() else -1.0
		var value: String = values[i] if i < values.size() else ""
		_spokes.append({"label": labels[i], "share": share, "value": value})
	queue_redraw()


func _draw() -> void:
	var count: int = _spokes.size()
	if count < 3:
		return
	var center: Vector2 = size / 2.0
	var radius: float = maxf(minf(size.x, size.y) / 2.0 - MARGIN, 10.0)
	for ring: float in RINGS:
		var outline: PackedVector2Array = PackedVector2Array()
		for i: int in range(count + 1):
			outline.append(center + _direction(i % count, count) * radius * ring)
		draw_polyline(outline, RIM if ring == 1.0 else GRID, 1.0, true)
	for i: int in range(count):
		draw_line(center, center + _direction(i, count) * radius, GRID)
	var corners: PackedVector2Array = PackedVector2Array()
	for i: int in range(count):
		var share: float = _spokes[i]["share"]
		corners.append(center + _direction(i, count) * radius * clampf(share, 0.0, 1.0))
	var fill: Color = Color(UiTheme.POWER_COLOR, 0.3)
	# A fan of triangles from the centre: a share of zero would break a triangulated polygon.
	for i: int in range(count):
		var next: int = (i + 1) % count
		draw_primitive(
			PackedVector2Array([center, corners[i], corners[next]]),
			PackedColorArray([fill, fill, fill]),
			PackedVector2Array()
		)
	corners.append(corners[0])
	draw_polyline(corners, UiTheme.POWER_COLOR, 2.0, true)
	var font: Font = get_theme_default_font()
	for i: int in range(count):
		_draw_label(font, center + _direction(i, count) * (radius + 12.0), i, count)


## The spoke's duration, share and value beside its end, pushed outward from the centre.
func _draw_label(font: Font, at: Vector2, index: int, count: int) -> void:
	var spoke: Dictionary = _spokes[index]
	var share: float = spoke["share"]
	var label: String = spoke["label"]
	var value: String = spoke["value"]
	var lines: Array[Array] = [
		[label, UiTheme.MUTED],
		["--" if share < 0.0 else "%d %%" % roundi(share * 100.0), UiTheme.TEXT],
		[value, UiTheme.MUTED],
	]
	var direction: Vector2 = _direction(index, count)
	var height: float = FONT_SIZE * 1.3 * lines.size()
	var top: float = at.y - height / 2.0 + direction.y * height / 2.0
	for row: int in range(lines.size()):
		var text: String = lines[row][0]
		var color: Color = lines[row][1]
		var width: float = font.get_string_size(text, HORIZONTAL_ALIGNMENT_LEFT, -1, FONT_SIZE).x
		# Left of the spoke on the left side, right of it on the right, centred at the top.
		var left: float = at.x - width / 2.0 + direction.x * width / 2.0
		var baseline: float = top + FONT_SIZE * 1.3 * (row + 1) - 3.0
		draw_string(
			font, Vector2(left, baseline), text, HORIZONTAL_ALIGNMENT_LEFT, -1, FONT_SIZE, color
		)


## The direction of spoke `index` of `count`, clockwise from the top.
static func _direction(index: int, count: int) -> Vector2:
	var angle: float = -PI / 2.0 + TAU * index / count
	return Vector2(cos(angle), sin(angle))
