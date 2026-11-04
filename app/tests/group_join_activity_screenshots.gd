class_name GroupJoinActivityScreenshots
extends RefCounted
## Comparable organizer/joiner course previews and an activity whose files are arriving (#508).


static func capture(
	tree: SceneTree, start: StartPage, group: GroupRidesTab, app: OrqwaApp, out: String
) -> void:
	var scroll: ScrollContainer = start.get("_group_page")
	var course: Dictionary = app.courses()[0]
	var course_path: String = course["path"]
	for locale: String in ["en", "de"]:
		TranslationServer.set_locale(locale)
		for view: String in ["organizer", "course", "course-workout", "download"]:
			var downloading: bool = view == "download"
			var status: Dictionary = {
				"host": view == "organizer",
				"phase": "downloading" if downloading else "ready",
				"late": view != "organizer",
				"name": course["name"],
				"course_path": "" if downloading else course["path"],
				"workout_id": "builtin:recovery-30" if view == "course-workout" else "",
				"received": 500000 if downloading else 0,
				"total": 2000000 if downloading else 0,
				"prepared": not downloading,
				"can_enter": not downloading,
				"roster":
				[
					{
						"id": 0,
						"name": "Alex",
						"organizer": true,
						"ready": true,
						"me": view == "organizer"
					},
					{"id": 1, "name": "Rider", "ready": false, "me": view != "organizer"},
				]
			}
			group._show_status(status)
			if view == "organizer":
				group._picker._select(group._picker._course, course_path)
				group._picker._select(group._picker._workout, "")
				group._picker._preview()
			scroll.scroll_vertical = 0
			await tree.create_timer(0.4).timeout
			var filename: String = "group-join-%s-%s.png" % [view, locale]
			tree.root.get_texture().get_image().save_png(out.path_join(filename))
			print("saved " + filename)
