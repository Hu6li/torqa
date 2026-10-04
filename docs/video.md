# Video courses

Ride a route along a real video (R17): the video plays as fast as you ride — climb slowly and
it slows down, sprint and it speeds up. The 3D world is not used on video courses.

## What you can import

**Import route, video or course…** on the Courses tab takes:

- **Your own videos with GPS** — GoPro recordings (`.mp4`/`.mov`) carry their GPS track inside
  the file (GPMF). Torqa reads it, builds the route from it and pairs every moment of the video
  with a position on it. Record with GPS switched on; a video without GPS is refused for now
  (manual sync points are planned).
- **Route videos made for Incyclist** — a folder with a video, a `.gpx` route and an `.xml`
  control file. Choose the **`.xml` file**. Many free route videos are published in this format,
  for example the library by Van Gestel offered from within Incyclist. Keep the three files
  together; the GPX timestamps (and the start frame in the `.xml`) place the route in the video.

The route goes through the usual import (elevation correction when online) and the course is
added to your library, marked **Video** on its card.

## Riding a video course

Open the course and press **Ride** as usual. Instead of the 3D world, the video fills the
screen, with your figures, map and elevation profile on top. Where you are on the route
decides the moment of the video: it plays at the speed you ride, stands still when you stop,
and blends smoothly from frame to frame even when you crawl up a steep climb.

## The video stays where it is

Videos are large, so a video course (`.tqc`) only **refers** to its video, it does not contain
it. Torqa looks for the video where it was imported from, then next to the course file (by name
and size). To move a video course to another computer, copy the `.tqc` and the video into the
same folder. If the video is missing, opening the course says which file to put there.

## Licences

Route videos belong to whoever filmed them; the free ones are usually for personal use only
(e.g. CC BY-NC-SA). Torqa does not ship or redistribute any videos — share courses together
with their videos only where the video's licence allows it.

Video is decoded with FFmpeg (LGPL), see [ADR 0010](adr/0010-video-decoding.md).
