| record | expected (slt) | PROJ result | exact | 12 sig. digits |
|---|---|---|---|---|
| st_3ddistance.slt #1 | `127.295059325 126.664256057` | `127.295059325 126.664256057` | True | True |
| st_3ddwithin.slt #1 | `false true` | `false true` | True | True |
| st_3dmaxdistance.slt #1 | `24383.7467488 22247.8472107` | `24383.7467488 22247.8472107` | True | True |
| st_area.slt #1 | `928.625 86.2724306193` | `928.625 86.2724306193` | True | True |
| st_area.slt #2 | `928.684404675 926.60976267 86.2776043949` | `928.684404675 926.60976267 86.2776043949` | True | True |
| st_buffer.slt #1 | `POLYGON((236057.5905746494 900908.7599186979,236028.30125...` | `POLYGON((236057.5905746494 900908.7599186979,236028.30125...` | True | True |
| st_distance.slt #1 | `167.441410065` | `167.441410065` | True | True |
| st_distance.slt #2 | `123.742351254` | `123.742351254` | True | True |
| st_distance.slt #3 | `123.797937878` | `123.797937878` | True | True |
| st_distance.slt #4 | `126.664256057` | `126.664256057` | True | True |
| st_distance_spheroid.slt #1 | `70454.92 70424.71 70438` | `70454.92 70424.71 70438` | True | True |
| st_distancesphere.slt #1 | `70424.71 70438 0.729 65871.18` | `70424.71 70438 0.729 65871.18` | True | True |
| st_inversetransformpipeline.slt #1 | `POINT(2 48.99999999999999)` | `POINT(2 48.99999999999999)` | True | True |
| st_inversetransformpipeline.slt #2 | `POINT(143.00000635638918 -36.999986706128176)` | `POINT(143.00000635638918 -36.999986706128176)` | True | True |
| st_length.slt #1 | `34309.4563576` | `34309.4563576` | True | True |
| st_point.slt #1 | `SRID=4326;POINT(-75.1890010541 39.9769998586)` | `SRID=4326;POINT(-75.1890010541 39.9769998586)` | True | True |
| st_setsrid.slt #1 | `SRID=3785;POINT(-13732990.8753 6178458.96425)` | `SRID=3785;POINT(-13732990.8753 6178458.96425)` | True | True |
| st_transform.slt #1 | `POLYGON((-71.1776848522251 42.3902896512903,-71.177684376...` | `POLYGON((-71.1776848522251 42.3902896512903,-71.177684376...` | True | True |
| st_transform.slt #2 | `SRID=4326;CIRCULARSTRING(-71.1776848522251 42.39028965129...` | `SRID=4326;CIRCULARSTRING(-71.1776848522251 42.39028965129...` | True | True |
| st_transform.slt #3 | `POLYGON((-140.9999840290591 73.42768865981984,-141 68,-17...` | `POLYGON((-140.9999840290591 73.42768865981984,-141 68,-17...` | True | True |
| st_transformpipeline.slt #1 | `POINT(426857.9877165967 5427937.523342293)` | `POINT(426857.9877165967 5427937.523342293)` | True | True |
| st_transformpipeline.slt #2 | `POINT(2 48.99999999999999)` | `POINT(2 48.99999999999999)` | True | True |

records agreeing to 12 significant digits: 22/22
