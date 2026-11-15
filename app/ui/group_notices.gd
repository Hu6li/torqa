class_name GroupNotices
extends RefCounted
## What the ride screen says of the group: when someone's connection to it drops, comes back
## or ends (R249), on the moments' card with their chime, in red for what went wrong and green
## for who is back; and who organizes it.


## The card for the core's `group_connection` notice: `[figure, line, accent, bar]`, the bar's
## text standing alone in the overlay; no line for a notice it does not know.
static func card(notice: Dictionary) -> Array:
	var friend: String = notice.get("name", "")
	var away: float = notice.get("away_s", 0.0)
	var red: Color = UiTheme.DANGER
	var green: Color = UiTheme.SUCCESS
	var cards: Dictionary[String, Array] = {
		"away":
		[
			friend,
			TranslationServer.translate(
				"Lost the connection. ORQWA keeps their place for five minutes."
			),
			red
		],
		"back":
		[
			friend,
			(
				TranslationServer.translate("Is back in the group after %s.")
				% GroupAdmissions.clock(away)
			),
			green
		],
		"gone": [friend, TranslationServer.translate("Has left the group."), red],
		"lost":
		[
			TranslationServer.translate("Connection lost"),
			TranslationServer.translate(
				"Your ride continues. ORQWA reconnects you for up to five minutes."
			),
			red
		],
		"reconnected":
		[
			TranslationServer.translate("Reconnected"),
			TranslationServer.translate("You are back with the group."),
			green
		],
	}
	var endings: Dictionary[String, String] = {
		"lost":
		TranslationServer.translate("Your ride continues alone. Join again in Group Rides."),
		"removed":
		TranslationServer.translate("The organizer removed you. Your ride continues alone."),
		"ended":
		TranslationServer.translate("The organizer ended the group. Your ride continues alone."),
	}
	var kind: String = notice.get("kind", "")
	var ending: String = notice.get("ending", "")
	if kind == "ended" and endings.has(ending):
		cards["ended"] = [TranslationServer.translate("Group ended"), endings[ending], red]
	if not cards.has(kind):
		return ["", "", green, ""]
	var shown: Array = cards[kind]
	var figure: String = shown[0]
	var line: String = shown[1]
	return shown + ["%s  ·  %s" % [figure, line]]


## The organizer's name in a group `status`, "" before one is known.
static func organizer(status: Dictionary) -> String:
	for rider: Dictionary in status.get("roster", []):
		if rider.get("organizer", false):
			var rider_name: String = rider.get("name", "")
			return rider_name
	return ""
