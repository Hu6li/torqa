extends SceneTree
## Headless checks of the ride's moments (R124): every moment has its line in both voices, in
## the rider's units, the card shows them one after the other and the summaries' line looks
## back on the ride.
## Run: godot --headless --path app -s res://tests/moment_smoke.gd

## One moment of each kind, as the core's `moment` signal sends them.
const MOMENTS: Array[Dictionary] = [
	{"kind": "climb_topped", "index": 1, "elapsed_s": 842.0, "grade_pct": 8.04},
	{"kind": "climb_record", "index": 0, "elapsed_s": 842.0, "gain_s": 12.0},
	{"kind": "halfway", "distance_m": 16050.0},
	{"kind": "one_to_go"},
	{"kind": "distance", "distance_m": 25000.0},
	{"kind": "first_ride", "course": "Oberalp"},
	{"kind": "power_peak", "duration_s": 300.0, "power_w": 311.6},
	{"kind": "step_held", "index": 3, "power_w": 262.0},
	{"kind": "streak", "rides": 3},
	{"kind": "week_plan", "rides": 3, "planned": 4},
	{"kind": "segment_done", "name": "Wabern sprint", "elapsed_s": 75.0},
	{"kind": "segment_record", "name": "Wabern sprint", "elapsed_s": 75.0, "gain_s": 4.0},
	{"kind": "segment_ahead", "name": "Wabern sprint", "distance_m": 187.4},
]

var _failed: bool = false


func _initialize() -> void:
	_run.call_deferred()


func _run() -> void:
	_lines()
	_recap()
	_chime()
	await _card_size()
	await _card()
	_group_notices()
	if not _failed:
		print("MOMENT SMOKE TEST PASSED")
	quit(1 if _failed else 0)


func _lines() -> void:
	for moment: Dictionary in MOMENTS:
		var kind: String = moment["kind"]
		for voice: String in ["casual", "plain", "off"]:
			for _i: int in range(6):
				var line: String = MomentLines.line(moment, voice, false)
				_check(not line.is_empty(), "%s has a %s line" % [kind, voice])
				_check(not "{" in line, "%s in %s fills every slot: %s" % [kind, voice, line])
	_check(_plain(MOMENTS[0], false) == "Climb 2 topped, 8.0 % average.", "climbs from 1")
	_check(_plain(MOMENTS[2], false) == "Halfway.", "plain facts")
	_check(_plain(MOMENTS[3], true) == "1 mi to go.", "in miles")
	_check(_plain(MOMENTS[4], false) == "25 km ridden.", "whole distances")
	_check(MomentLines.figure(MOMENTS[2], false) == "16.1 km", "a distance figure")
	_check(MomentLines.distance(420.0, false) == "420 m", "a short distance")
	_check(MomentLines.figure(MOMENTS[1], false) == "14:02", "a time figure")
	_check(_plain(MOMENTS[6], false) == "Best 5 min power: 312 W.", "a power record")
	_check(MomentLines.figure(MOMENTS[6], false) == "312 W", "a power figure")
	_check(MomentLines.span(5.0) == "5 s" and MomentLines.span(3600.0) == "1 h", "spans")
	_check(_plain(MOMENTS[9], false) == "Ride 3 of 4 planned this week.", "the plan")
	var done: Dictionary = MOMENTS[9].duplicate()
	done["rides"] = 4
	_check(_plain(done, false) == "All 4 planned rides this week done.", "the plan done")
	done["rides"] = 5
	_check(_plain(done, false) == "Ride 5 this week, 4 planned.", "beyond the plan")
	var seeded: String = MomentLines.line(MOMENTS[8], "casual", false, 7)
	for _i: int in range(4):
		_check(MomentLines.line(MOMENTS[8], "casual", false, 7) == seeded, "a seed keeps the line")
	_check(MomentLines.line({"kind": "unknown"}, "casual", false).is_empty(), "unknown kinds")
	# The rider's own segments (R302) by the name they gave them.
	_check(
		_plain(MOMENTS[11], false) == "Wabern sprint in 1:15, a personal record by 0:04.",
		"a segment record"
	)
	_check(MomentLines.figure(MOMENTS[10], false) == "1:15", "a segment's time")
	# Its start announced in round figures of the rider's units (R303).
	_check(_plain(MOMENTS[12], false) == "Wabern sprint starts in 190 m.", "a segment ahead")
	_check(
		_plain(MOMENTS[12], true) == "Wabern sprint starts in 610 ft.", "a segment ahead in feet"
	)
	_check(MomentLines.figure(MOMENTS[12], false) == "190 m", "how far a segment's start is")
	_check(MomentLines.approach(3.0, false) == "10 m", "never 0 m before the start")
	var to_go: Array[float] = [187.4]
	var count: Callable = MomentLines.countdown(
		MOMENTS[12], "plain", false, func() -> float: return to_go[0]
	)
	var at_first: PackedStringArray = count.call()
	_check(at_first[1] == "Wabern sprint starts in 190 m.", "counting from %s" % [at_first])
	to_go[0] = 42.0
	var nearer: PackedStringArray = count.call()
	_check(nearer == PackedStringArray(["40 m", "Wabern sprint starts in 40 m."]), "counting down")
	to_go[0] = 0.0
	var there: PackedStringArray = count.call()
	_check(there.is_empty(), "nothing left to count at the start")
	var casual: Callable = MomentLines.countdown(
		MOMENTS[12], "casual", false, func() -> float: return to_go[0] + 100.0
	)
	var once: PackedStringArray = casual.call()
	for _i: int in range(6):
		var again: PackedStringArray = casual.call()
		_check(again == once, "one casual line for the whole countdown")


func _plain(moment: Dictionary, imperial: bool) -> String:
	return MomentLines.line(moment, "plain", imperial)


## The summaries' line looks back on the ride: no "enjoy the new roads" once it is over.
func _recap() -> void:
	var recap: RideRecap = RideRecap.new()
	root.add_child(recap)
	recap.show_recap(MOMENTS[5], "plain", false, 3)
	_check(recap.visible and recap.text == "First ride on Oberalp.", "first ride: %s" % recap.text)
	for seed: int in range(4):
		recap.show_recap(MOMENTS[5], "casual", false, seed)
		_check(not recap.text.contains("enjoy"), "told looking back: %s" % recap.text)
		_check(recap.text.contains("Oberalp"), "naming the course: %s" % recap.text)
	recap.show_recap(MOMENTS[8], "off", false, 0)
	_check(recap.text == "Ride 3 this week.", "plain when the voice is off: %s" % recap.text)
	recap.show_recap({}, "casual", false, 0)
	_check(not recap.visible, "nothing to say, no line")
	recap.queue_free()


func _chime() -> void:
	var wav: AudioStreamWAV = MomentChime.make()
	var loudest: int = 0
	for i: int in range(0, wav.data.size(), 2):
		loudest = maxi(loudest, absi(wav.data.decode_s16(i)))
	_check(is_equal_approx(wav.get_length(), MomentChime.SECONDS), "the chime's length")
	_check(loudest > 3000 and loudest < 32767, "a chime heard but not clipped: %d" % loudest)
	_check(absi(wav.data.decode_s16(0)) < 100, "the chime starts without a click")


## The first card of a ride used to measure its line before it had a width and ran off the
## bottom of the screen; every card fits the window, a narrow one too.
func _card_size() -> void:
	var longest: String = ""
	for kind: String in MomentLines.LINES:
		var lines: Array = MomentLines.LINES[kind]
		for line: String in lines[0]:
			if line.length() > longest.length():
				longest = line
	for window: Vector2i in [Vector2i(1600, 900), Vector2i(640, 480)]:
		root.size = window
		var holder: Control = Control.new()
		holder.set_anchors_and_offsets_preset(Control.PRESET_FULL_RECT)
		root.add_child(holder)
		await process_frame
		var card: MomentCard = MomentCard.new()
		holder.add_child(card)
		card.tell("14:02", longest)
		_check(card.size.y < 220.0, "a short card in %s: %s" % [window, card.size])
		_check(card.size.x <= window.x, "as wide as the window at most: %s" % card.size)
		holder.queue_free()
		await process_frame


func _card() -> void:
	var card: MomentCard = MomentCard.new()
	root.add_child(card)
	var told: Array[String] = []
	card.shown.connect(func(text: String) -> void: told.append(text))
	card.tell("14:02", "First line.")
	card.tell("", "Second line.")
	card.tell("", "")
	_check(card.visible and card.is_showing(), "a card shows")
	_check(card.current() == PackedStringArray(["14:02", "First line."]), "the first first")
	_check(told == ["14:02  ·  First line."], "told once: %s" % [told])
	await create_timer(MomentCard.SHOW_SECONDS + MomentCard.SLIDE_SECONDS * 2.0 + 0.2).timeout
	_check(card.current() == PackedStringArray(["", "Second line."]), "then the next")
	_check(told.size() == 2, "an empty line is not shown: %s" % [told])
	card.clear()
	_check(not card.visible and not card.is_showing(), "cleared")
	# A segment ahead is a heads-up, not an achievement: no burst, and before the cards still
	# waiting, though after the one on screen (R303).
	var burst: CPUParticles2D = card.get("_burst")
	card.tell("190 m", "A segment ahead.", UiTheme.SEGMENT_COLOR, true)
	_check(card.is_showing() and not burst.emitting, "a heads-up without a burst")
	card.clear()
	told.clear()
	card.tell("14:02", "A record.")
	_check(burst.emitting, "an achievement with its burst")
	card.tell("", "Halfway.")
	card.tell("120 m", "The next segment.", UiTheme.SEGMENT_COLOR, true)
	await create_timer(MomentCard.SHOW_SECONDS + MomentCard.SLIDE_SECONDS * 2.0 + 0.2).timeout
	_check(
		told == ["14:02  ·  A record.", "120 m  ·  The next segment."],
		"the heads-up goes before the waiting card: %s" % [told]
	)
	card.clear()
	# Its distance counts down while it shows, and it goes once the rider is there.
	var left: Array[float] = [120.0]
	var live: Callable = func() -> PackedStringArray:
		if left[0] <= 0.0:
			return PackedStringArray()
		var now: String = MomentLines.approach(left[0], false)
		return PackedStringArray([now, "Starts in %s." % now])
	card.tell("120 m", "Starts in 120 m.", UiTheme.SEGMENT_COLOR, true, live)
	left[0] = 64.0
	await process_frame
	await process_frame
	_check(
		card.current() == PackedStringArray(["60 m", "Starts in 60 m."]),
		"the distance counts down: %s" % [card.current()]
	)
	left[0] = -1.0
	await create_timer(MomentCard.SLIDE_SECONDS + 0.2).timeout
	_check(not card.is_showing(), "gone at the start, before its time is up")
	card.clear()
	card.queue_free()


## Group connections take the card in red when one dropped and green when back (R249).
func _group_notices() -> void:
	var away: Array = GroupNotices.card({"kind": "away", "names": PackedStringArray(["Ben"])})
	var figure: String = away[0]
	var red: Color = away[2]
	var bar: String = away[3]
	_check(figure == "Ben" and red == UiTheme.DANGER, "a dropped friend in red: %s" % [away])
	_check(bar.begins_with("Ben  ·  "), "the overlay names them: %s" % [away])
	var back: Array = GroupNotices.card(
		{"kind": "back", "names": PackedStringArray(["Ben"]), "away_s": 72.0}
	)
	var green: Color = back[2]
	var since: String = back[1]
	_check(green == UiTheme.SUCCESS, "a friend back in green")
	_check(since.contains("1:12"), "with how long they were away: %s" % since)
	var removed: String = GroupNotices.card({"kind": "ended", "ending": "removed"})[1]
	_check(removed.contains("removed"), "this rider reads why: %s" % removed)
	# Friends who drop together come on one card, plural (R249).
	var both: Array = GroupNotices.card(
		{"kind": "away", "names": PackedStringArray(["Ben", "Carla"])}
	)
	var pair: String = both[0]
	var places: String = both[1]
	_check(pair == "Ben and Carla" and places.contains("places"), "together: %s" % [both])
	var many: String = GroupNotices.together(PackedStringArray(["A", "B", "C", "D", "E"]))
	_check(many == "A, B and 3 more", many)
	var everyone: String = (
		GroupNotices
		. card({"kind": "away", "names": PackedStringArray(["Ben", "Carla"]), "everyone": true})[1]
	)
	_check(everyone.contains("own network"), "the organizer whom everyone dropped: %s" % everyone)
	var unknown: String = GroupNotices.card({"kind": "joined"})[1]
	_check(unknown.is_empty(), "nothing for an unknown notice")
	var card: MomentCard = MomentCard.new()
	root.add_child(card)
	card.callv("tell", away.slice(0, 3))
	var style: StyleBoxFlat = card.get("_style")
	_check(style.border_color.is_equal_approx(Color(UiTheme.DANGER, 0.7)), "the card in red")
	card.clear()
	card.queue_free()


func _check(condition: bool, what: String) -> void:
	if not condition:
		_failed = true
		push_error("FAILED: " + what)
