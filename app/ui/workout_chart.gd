class_name WorkoutChart
extends Control
## A structured workout's steps over time (R21): each step's power as a block, coloured by its
## power zone, free steps as low grey blocks, and an FTP test's all-out parts (#125) to the
## top. During the workout the part done is dimmed.

## Upper bounds of power zones 1–6 as a share of FTP (Coggan), as in the core's profile.
const ZONE_BOUNDS: Array[float] = [0.55, 0.75, 0.90, 1.05, 1.20, 1.50]
const FREE_COLOR: Color = Color(1, 1, 1, 0.12)
## Free steps show at this share of FTP: low, as they ask nothing.
const FREE_SHARE: float = 0.3
## All-out parts ask everything the rider has, but no set power: see-through.
const ALL_OUT_ALPHA: float = 0.55
const DONE_SHADE: Color = Color(0, 0, 0, 0.45)

## Time into the workout, in seconds; the part before it is dimmed. Negative: none.
var progress_s: float = -1.0:
	set(value):
		progress_s = value
		queue_redraw()

var _steps: Array = []
var _ftp_w: float = 200.0
var _duration: float = 1.0


## Shows `steps` (`TorqaApp.workouts()[i]["steps"]`) for a rider with `ftp_w`.
func set_steps(steps: Array, ftp_w: float) -> void:
	_steps = steps
	_ftp_w = maxf(ftp_w, 1.0)
	_duration = 0.0
	for step: Dictionary in steps:
		var duration: float = step["duration_s"]
		_duration += duration
	_duration = maxf(_duration, 1.0)
	queue_redraw()


## The power at the top of the chart: a little above the hardest step, at least 120 % of FTP.
func top_w() -> float:
	var top: float = _ftp_w * 1.2
	for step: Dictionary in _steps:
		if step["from_w"] != null:
			var from: float = step["from_w"]
			var to: float = step["to_w"]
			top = maxf(top, maxf(from, to) * 1.05)
	return top


func _draw() -> void:
	if _steps.is_empty():
		return
	var top: float = top_w()
	var start: float = 0.0
	for step: Dictionary in _steps:
		var duration: float = step["duration_s"]
		var left: float = start / _duration * size.x
		var right: float = (start + duration) / _duration * size.x
		var from_w: float = _ftp_w * FREE_SHARE
		var to_w: float = from_w
		var color: Color = FREE_COLOR
		if step["from_w"] != null:
			from_w = step["from_w"]
			to_w = step["to_w"]
			color = _zone_color((from_w + to_w) / 2.0)
		elif step.get("all_out", false):
			from_w = top
			to_w = top
			color = _zone_color(top)
			color.a = ALL_OUT_ALPHA
		var bottom: float = size.y
		draw_colored_polygon(
			PackedVector2Array(
				[
					Vector2(left, bottom),
					Vector2(left, bottom - from_w / top * size.y),
					Vector2(right, bottom - to_w / top * size.y),
					Vector2(right, bottom),
				]
			),
			color
		)
		start += duration
	if progress_s >= 0.0:
		var done: float = clampf(progress_s / _duration, 0.0, 1.0) * size.x
		draw_rect(Rect2(0.0, 0.0, done, size.y), DONE_SHADE)
		draw_line(Vector2(done, 0.0), Vector2(done, size.y), UiTheme.TEXT, 2.0)


func _zone_color(watts: float) -> Color:
	var share: float = watts / _ftp_w
	var zone: int = 0
	while zone < ZONE_BOUNDS.size() and share > ZONE_BOUNDS[zone]:
		zone += 1
	var color: Color = UiTheme.POWER_ZONES[zone][1]
	return color
