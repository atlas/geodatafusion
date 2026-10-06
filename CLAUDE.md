# geodatafusion

Spatial UDFs for Apache DataFusion, modelled on PostGIS.

- **[STYLE_GUIDE.md](STYLE_GUIDE.md)** applies to every change: code, comments, tests and docs.
  Read it before writing code.
- **[DEVELOP.md](DEVELOP.md)** covers building, testing, formatting and linting.
- **[plans/README.md](plans/README.md)** is the PostGIS parity plan. Find a function's group in
  [plans/inventory.md](plans/inventory.md), then follow that group's plan and template. The
  README's cross-group decisions win over the individual plans.
- To test a function against PostGIS, use the `postgis-parity-tests` skill (`cargo slt <function>`).
