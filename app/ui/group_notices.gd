class_name GroupNotices
extends RefCounted
## What the ride screen says of the group: when someone's connection to it drops, comes back
## or ends (R249), on the moments' card with their chime, in red for what went wrong and green
## for who is back; friends who drop together come on one card. And who organizes it.

## Per notice kind: the heading (for friends, their names fill it), the line for one friend,
## the line for several, and whether it went wrong.
# i18n-begin
const CARDS: Dictionary[String, Array] = {
	"away":
	[
		"",
		"Lost the connection. Their place is kept for five minutes.",
		"Lost the connection. Their places are kept for five minutes.",
		true
	],
	"everyone":
	[
		"Everyone lost the connection",
		"",
		"Your own network has probably dropped. Their places are kept for five minutes.",
		true
	],
	"back": ["", "Is back in the group after {away}.", "Are back in the group.", false],
	"gone": ["", "Has left the group.", "Have left the group.", true],
	"lost":
	[
		"Connection lost",
		"Your ride continues. You are reconnected automatically for up to five minutes.",
		"",
		true
	],
	"reconnected": ["Reconnected", "You are back with the group.", "", false],
	"lost_ended":
	["Group ended", "Your ride continues alone. Join again in Group Rides.", "", true],
	"removed_ended":
	["Group ended", "The organizer removed you. Your ride continues alone.", "", true],
	"ended_ended":
	["Group ended", "The organizer ended the group. Your ride continues alone.", "", true],
}
# i18n-end


## The card for the core's `group_connection` notice: `[figure, line, accent, bar]`, the bar's
## text standing alone in the overlay; no line for a notice it does not know.
static func card(notice: Dictionary) -> Array:
	var kind: String = notice.get("kind", "")
	var names: PackedStringArray = notice.get("names", PackedStringArray())
	if kind == "ended":
		var ending: String = notice.get("ending", "")
		kind = ending + "_ended"
	if kind == "away" and notice.get("everyone", false):
		kind = "everyone"
	if not CARDS.has(kind):
		return ["", "", UiTheme.SUCCESS, ""]
	var shown: Array = CARDS[kind]
	var heading: String = shown[0]
	var figure: String = together(names)
	if names.is_empty() or kind == "everyone":
		figure = TranslationServer.translate(heading)
	var text: String = shown[2] if names.size() > 1 or kind == "everyone" else shown[1]
	var away: float = notice.get("away_s", 0.0)
	var line: String = TranslationServer.translate(text).format(
		{"away": GroupAdmissions.clock(away)}
	)
	var wrong: bool = shown[3]
	var accent: Color = UiTheme.DANGER if wrong else UiTheme.SUCCESS
	return [figure, line, accent, "%s  ·  %s" % [figure, line]]


## Friends named together: "Ben", "Ben and Carla", "Ben, Carla and Dana", then "and 2 more".
static func together(names: PackedStringArray) -> String:
	if names.size() <= 1:
		return "".join(names)
	if names.size() > 3:
		var first: String = ", ".join(names.slice(0, 2))
		return TranslationServer.translate("%s and %d more") % [first, names.size() - 2]
	var last: String = names[names.size() - 1]
	return (
		TranslationServer.translate("%s and %s")
		% [", ".join(names.slice(0, names.size() - 1)), last]
	)


## The organizer's name in a group `status`, "" before one is known.
static func organizer(status: Dictionary) -> String:
	for rider: Dictionary in status.get("roster", []):
		if rider.get("organizer", false):
			var rider_name: String = rider.get("name", "")
			return rider_name
	return ""
