class_name MomentLines
extends RefCounted
## What the app says of a moment of the ride (R124), in the rider's voice: the warm, cheering
## casual voice picks one of its lines at random, the plain one states the facts. All lines
## live here so they are easy to refresh.

const KM_PER_MILE: float = 1.609344
const METERS_PER_FOOT: float = 0.3048

## Per moment kind (as the core's `moment` signal names it, or a variant `_kind()` picks): the
## casual lines, then the plain one. `{…}` are filled in by `line()`.
# i18n-begin
const LINES: Dictionary[String, Array] = {
	"climb_topped":
	[
		[
			"Climb {n} is done and it averaged {grade}. Well climbed!",
			"You're over the top of climb {n}, which averaged {grade}. Get your breath back!",
		],
		"Climb {n} topped, {grade} average.",
	],
	"climb_record":
	[
		[
			"{time} on climb {n} is your best time up there yet!",
			"You've never been up climb {n} faster than that, {gain} quicker than before!",
		],
		"Climb {n} in {time}, a personal record by {gain}.",
	],
	"segment_done":
	[
		[
			"{name} is done in {time}. Nicely ridden!",
			"That's {name} behind you, in {time}!",
		],
		"{name} in {time}.",
	],
	"segment_record":
	[
		[
			"{time} on {name} is your fastest yet!",
			"You've never ridden {name} faster, {gain} quicker than your best!",
		],
		"{name} in {time}, a personal record by {gain}.",
	],
	"segment_ahead":
	[
		[
			"{name} starts in {distance}, get ready!",
			"{name} is coming up in {distance}. Time to wind it up!",
		],
		"{name} starts in {distance}.",
	],
	"halfway":
	[
		[
			"You're halfway there and going really well!",
			"Half the ride is done already, keep it rolling!",
		],
		"Halfway.",
	],
	"one_to_go":
	[
		[
			"Just {left} to go, give it everything!",
			"Only {left} left, so bring it home strong!",
		],
		"{left} to go.",
	],
	"distance":
	[
		[
			"{distance} done and you're still going strong!",
			"That's {distance} on the clock already, great going!",
		],
		"{distance} ridden.",
	],
	"power_peak":
	[
		[
			"{power} for {duration} is a new best for you!",
			"That was your best {duration} ever, at {power}!",
		],
		"Best {duration} power: {power}.",
	],
	"step_held":
	[
		[
			"You held that step right on target, nicely done!",
			"That step was spot on the whole way through!",
		],
		"Step completed on target.",
	],
	"streak":
	[
		[
			"That's {rides} rides this week, keep it up!",
			"{rides} rides this week already, you're on a roll!",
		],
		"Ride {rides} this week.",
	],
	"week_plan":
	[
		[
			"That's ride {rides} of the {planned} you planned this week, nice work!",
			"{rides} of your {planned} rides this week are done, keep going!",
		],
		"Ride {rides} of {planned} planned this week.",
	],
	"week_plan_done":
	[
		[
			"That's all {planned} rides you planned this week. Well done!",
			"Every ride you planned this week is done, all {planned} of them!",
		],
		"All {planned} planned rides this week done.",
	],
	"week_plan_beyond":
	[
		[
			"That's {rides} rides this week, even more than the {planned} you planned!",
			"Ride {rides} this week puts you beyond your plan of {planned}. Brilliant!",
		],
		"Ride {rides} this week, {planned} planned.",
	],
	"first_ride":
	[
		[
			"Your first ride on {course}, so enjoy the new roads!",
			"You've never ridden {course} before. Have fun exploring it!",
		],
		"Your first ride on {course}.",
	],
	"first_ride_done":
	[
		[
			"Your first ride on {course} is done, and now you know the way!",
			"That was your first time on {course}. Next time you'll know what's coming!",
		],
		"First ride on {course}.",
	],
}
# i18n-end


## The line for `moment` (`{kind, …}` as the core sends it) in `voice` ("casual", "plain";
## "off" speaks plainly too, for the summaries' line); empty for an unknown kind. The
## casual voice picks one of its lines at random, or by `seed` when one is given, so a ride's
## summary says the same each time it opens.
static func line(moment: Dictionary, voice: String, imperial: bool, seed: int = -1) -> String:
	var kind: String = _kind(moment)
	if not LINES.has(kind):
		return ""
	var lines: Array = LINES[kind]
	var text: String
	if voice == "casual":
		var casual: Array = lines[0]
		text = casual.pick_random() if seed < 0 else casual[seed % casual.size()]
	else:
		text = lines[1]
	return TranslationServer.translate(text).format(_values(moment, imperial))


## The lines for `moment`: some kinds are told differently by their figures.
static func _kind(moment: Dictionary) -> String:
	var kind: String = moment.get("kind", "")
	match kind:
		"week_plan":
			var rides: int = moment.get("rides", 0)
			var planned: int = moment.get("planned", 0)
			if rides == planned:
				return "week_plan_done"
			return "week_plan_beyond" if rides > planned else kind
	return kind


## The figure a card shows large for `moment`, e.g. "14:02" or "25 km"; empty for none.
static func figure(moment: Dictionary, imperial: bool) -> String:
	match moment.get("kind", ""):
		"climb_topped", "climb_record", "segment_done", "segment_record":
			var elapsed: float = moment["elapsed_s"]
			return UiTheme.duration(elapsed)
		"halfway", "distance":
			var meters: float = moment["distance_m"]
			return distance(meters, imperial)
		"segment_ahead":
			var ahead: float = moment["distance_m"]
			return approach(ahead, imperial)
		"one_to_go":
			return _one(imperial)
		"power_peak", "step_held":
			var power: float = moment["power_w"]
			return "%d W" % roundi(power)
	return ""


## `meters` in the rider's units: "32.1 km", whole ones without a decimal ("25 km"), short
## ones in metres or feet ("400 m").
static func distance(meters: float, imperial: bool) -> String:
	if meters < 1000.0:
		return climbing(meters, imperial)
	var value: float = meters / 1000.0 / (KM_PER_MILE if imperial else 1.0)
	var unit: String = "mi" if imperial else "km"
	var rounded: float = snappedf(value, 0.1)
	if is_equal_approx(rounded, roundf(rounded)):
		return "%d %s" % [roundi(rounded), unit]
	return "%.1f %s" % [rounded, unit]


## A heads-up's `[figure, line]` afresh for `MomentCard.tell`, with `left` the metres to go
## now; none once there. It keeps one line for the whole countdown, not a new one each frame.
static func countdown(
	moment: Dictionary, voice: String, imperial: bool, left: Callable
) -> Callable:
	var seed: int = randi() % 1024
	return func() -> PackedStringArray:
		var meters: float = left.call()
		if meters <= 0.0:
			return PackedStringArray()
		var now: Dictionary = moment.duplicate()
		now["distance_m"] = meters
		return PackedStringArray([figure(now, imperial), line(now, voice, imperial, seed)])


## How far a segment's start is, to the nearest 10 m or 10 ft: the rider is moving, so more
## precision would only be wrong by the time it is read.
static func approach(meters: float, imperial: bool) -> String:
	if imperial:
		return "%d ft" % maxi(10, roundi(meters / METERS_PER_FOOT / 10.0) * 10)
	return "%d m" % maxi(10, roundi(meters / 10.0) * 10)


## A power record's duration: "5 s", "20 min", "1 h".
static func span(seconds: float) -> String:
	var whole: int = roundi(seconds)
	if whole < 60:
		return "%d s" % whole
	if whole < 3600:
		return "%d min" % (whole / 60)
	return "%d h" % (whole / 3600)


## `meters` of climbing in the rider's units.
static func climbing(meters: float, imperial: bool) -> String:
	if imperial:
		return "%d ft" % roundi(meters / METERS_PER_FOOT)
	return "%d m" % roundi(meters)


static func _one(imperial: bool) -> String:
	return "1 mi" if imperial else "1 km"


static func _values(moment: Dictionary, imperial: bool) -> Dictionary:
	var values: Dictionary = {"left": _one(imperial)}
	if moment.has("index"):
		var index: int = moment["index"]
		values["n"] = index + 1
	if moment.has("elapsed_s"):
		var elapsed: float = moment["elapsed_s"]
		values["time"] = UiTheme.duration(elapsed)
	if moment.has("gain_s"):
		var gain: float = moment["gain_s"]
		values["gain"] = UiTheme.duration(gain)
	if moment.has("grade_pct"):
		var grade: float = moment["grade_pct"]
		values["grade"] = "%.1f %%" % grade
	if moment.has("distance_m"):
		var meters: float = moment["distance_m"]
		var ahead: bool = moment.get("kind", "") == "segment_ahead"
		values["distance"] = approach(meters, imperial) if ahead else distance(meters, imperial)
	if moment.has("power_w"):
		var power: float = moment["power_w"]
		values["power"] = "%d W" % roundi(power)
	if moment.has("duration_s"):
		var seconds: float = moment["duration_s"]
		values["duration"] = span(seconds)
	if moment.has("rides"):
		values["rides"] = moment["rides"]
	if moment.has("planned"):
		values["planned"] = moment["planned"]
	if moment.has("course"):
		values["course"] = moment["course"]
	if moment.has("name"):
		values["name"] = moment["name"]
	return values
