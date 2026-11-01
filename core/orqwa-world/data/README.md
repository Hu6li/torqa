# Regional architecture outlines

`asia.json` contains the unchanged MultiPolygon coordinates for country-coder features in
UN M49 region `142` (Asia): 65 features expanded to 80 polygons. The source is
[`country-coder` borders.json](https://github.com/rapideditor/country-coder/blob/c30d4314809cb8a446c3d42181cd370a1a424056/src/data/borders.json),
revision `c30d4314809cb8a446c3d42181cd370a1a424056`, retrieved 2026-11-01. Coordinates are
longitude/latitude; closed outer rings are followed by any holes. The set includes the
source's country and territory partitions across East, Southeast, South, Central and West
Asia, including Asian Russia.

This is a simplified regional classifier, including offshore areas, rather than a surveyed
coastline or a claim about the architecture of each individual building. It selects the
starter regional archetypes without network access, independently for each building.

The source is ISC-licensed, copyright 2026 country-coder contributors. The unchanged
[upstream licence](../../orqwa-osm/data/country-coder-LICENSE.md) is already bundled for
the traffic-territory outlines from the same source. No country-coder code is included.

`nordic.json` retains the geometries for Denmark, Finland, Iceland, Norway and Sweden, plus
the Faroe Islands, Åland, Svalbard and Jan Mayen: nine country/territory polygons from the
same revision. Greenland and Bouvet Island are excluded. The Nordic classifier is explicit
rather than treating all of Northern Europe as Nordic.

`france.json` is a coarse hand-drawn outline of metropolitan France and Corsica for regional
selection. It deliberately excludes overseas France and other French-speaking countries;
the simplified line is suitable for a scenery heuristic, not boundary display or legal use.
