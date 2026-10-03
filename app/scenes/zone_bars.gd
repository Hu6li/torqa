class_name ZoneBars
extends VBoxContainer
## Time in training zones as labelled horizontal bars.


## `seconds` per zone; `zones` holds `[name, colour]` per zone, as in `UiTheme`.
func set_zones(seconds: PackedFloat64Array, zones: Array[Array]) -> void:
	for child: Node in get_children():
		child.queue_free()
	var total: float = 0.0
	for value: float in seconds:
		total += value
	for i: int in range(mini(seconds.size(), zones.size())):
		var zone_name: String = zones[i][0]
		var color: Color = zones[i][1]
		var row: HBoxContainer = HBoxContainer.new()
		row.add_theme_constant_override("separation", 10)
		var label: Label = Label.new()
		label.text = "Z%d %s" % [i + 1, tr(zone_name)]
		label.custom_minimum_size = Vector2(130, 0)
		label.add_theme_font_size_override("font_size", 12)
		row.add_child(label)
		var bar: ProgressBar = ProgressBar.new()
		bar.show_percentage = false
		bar.max_value = maxf(total, 1.0)
		bar.value = seconds[i]
		bar.size_flags_horizontal = Control.SIZE_EXPAND_FILL
		bar.size_flags_vertical = Control.SIZE_SHRINK_CENTER
		bar.custom_minimum_size = Vector2(0, 10)
		var fill: StyleBoxFlat = StyleBoxFlat.new()
		fill.bg_color = color
		fill.set_corner_radius_all(4)
		bar.add_theme_stylebox_override("fill", fill)
		row.add_child(bar)
		var time: Label = Label.new()
		var minutes: int = roundi(seconds[i]) / 60
		time.text = "%d:%02d" % [minutes, roundi(seconds[i]) % 60]
		time.custom_minimum_size = Vector2(48, 0)
		time.horizontal_alignment = HORIZONTAL_ALIGNMENT_RIGHT
		time.add_theme_font_size_override("font_size", 12)
		row.add_child(time)
		add_child(row)
