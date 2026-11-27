class_name SegmentPanel
extends PanelContainer
## The rider's own segment during a ride (R302), on the HUD only (no banners in the world,
## R44/R89): from a little ahead its name and how far to its start; on it the time so far, the
## distance left, and against the rider's best, how far ahead or behind they are at this point
## if they rode it evenly then. Hidden away from segments.

## Before this share of the segment the gap to the best says little: the start is a sprint.
const GAP_FROM: float = 0.1

var imperial: bool = false
var _title: Label = UiTheme.caption("")
var _progress: Label = UiTheme.value(14)
var _best: Label = Label.new()


func _init() -> void:
	var rows: VBoxContainer = VBoxContainer.new()
	rows.add_theme_constant_override("separation", 2)
	_title.add_theme_color_override("font_color", UiTheme.SEGMENT_COLOR)
	rows.add_child(_title)
	rows.add_child(_progress)
	_best.auto_translate_mode = Node.AUTO_TRANSLATE_MODE_DISABLED
	_best.add_theme_font_size_override("font_size", 13)
	_best.add_theme_color_override("font_color", UiTheme.MUTED)
	rows.add_child(_best)
	var style: StyleBoxFlat = UiTheme.panel()
	style.content_margin_top = 10
	style.content_margin_bottom = 10
	add_theme_stylebox_override("panel", style)
	add_child(rows)
	hide()


## Shows `segment` (`ride_state()["segment"]`); null hides the panel.
func show_segment(segment: Variant) -> void:
	if segment == null:
		hide()
		return
	var info: Dictionary = segment
	var segment_name: String = info["name"]
	# The rider's own name for it, never translated.
	_title.text = tr("Segment  ·  %s") % segment_name
	var best: Variant = info["best_s"]
	if info["ahead_m"] != null:
		var ahead: float = info["ahead_m"]
		_progress.text = tr("Starts in %s") % ClimbPanel.short_distance(ahead, imperial)
		_best.text = ""
		if best != null:
			var best_s: float = best
			_best.add_theme_color_override("font_color", UiTheme.MUTED)
			_best.text = tr("Best %s") % UiTheme.duration(best_s)
	else:
		var length: float = info["length_m"]
		var ridden: float = info["ridden_m"]
		var elapsed: float = info["elapsed_s"]
		_progress.text = (
			tr("%s · %s to go")
			% [UiTheme.duration(elapsed), ClimbPanel.short_distance(length - ridden, imperial)]
		)
		_best.text = _against_best(best, elapsed, ridden / maxf(length, 1.0))
	_best.visible = not _best.text.is_empty()
	show()


## "Best 2:31 · −0:04": the best time and the gap to riding it evenly, green when ahead.
func _against_best(best: Variant, elapsed: float, share: float) -> String:
	if best == null:
		_best.add_theme_color_override("font_color", UiTheme.MUTED)
		return tr("First time on it")
	var best_s: float = best
	var text: String = tr("Best %s") % UiTheme.duration(best_s)
	if share < GAP_FROM:
		_best.add_theme_color_override("font_color", UiTheme.MUTED)
		return text
	var gap: float = elapsed - best_s * share
	var ahead: bool = gap < 0.0
	_best.add_theme_color_override("font_color", UiTheme.SUCCESS if ahead else UiTheme.DANGER)
	return "%s · %s%s" % [text, "−" if ahead else "+", UiTheme.duration(absf(gap))]
