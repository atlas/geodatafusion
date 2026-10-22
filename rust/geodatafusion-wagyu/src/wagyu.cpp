// A C entry point for the Wagyu polygon clipper, called from lib.rs.
//
// Wagyu is a header-only C++ library, so it can't be called from Rust directly. This file
// clips a set of rings to a box with Wagyu's quick_clip and unions them with its Vatti
// clipper, and hands the result back through callbacks, one per point, ring end and polygon
// end, so that no memory crosses the language boundary. C++ exceptions are caught here.

#include <cstddef>
#include <cstdint>
#include <cstring>
#include <exception>

#include <mapbox/geometry/box.hpp>
#include <mapbox/geometry/multi_polygon.hpp>
#include <mapbox/geometry/polygon.hpp>
#include <mapbox/geometry/wagyu/quick_clip.hpp>
#include <mapbox/geometry/wagyu/wagyu.hpp>

namespace {

using coordinate = std::int32_t;
using point = mapbox::geometry::point<coordinate>;
using linear_ring = mapbox::geometry::linear_ring<coordinate>;
using multi_polygon = mapbox::geometry::multi_polygon<coordinate>;
using box = mapbox::geometry::box<coordinate>;

void copy_message(char const* message, char* error, std::size_t error_len) {
    if (error == nullptr || error_len == 0) {
        return;
    }
    std::strncpy(error, message, error_len - 1);
    error[error_len - 1] = '\0';
}

} // namespace

extern "C" {

typedef void (*gdf_wagyu_point_fn)(void* context, std::int32_t x, std::int32_t y);
typedef void (*gdf_wagyu_end_fn)(void* context);

// Clips the rings to the box and unions them with the given fill rule.
//
// `coords` holds x, y pairs; ring i ends before the point at `ring_ends[i]`. The result is
// reported polygon by polygon, the shell first: `on_point` for every point, `on_ring_end`
// after each ring and `on_polygon_end` after each polygon. Returns 0 on success, or 1 with a
// message in `error` if Wagyu threw.
int gdf_wagyu_clip(std::int32_t const* coords,
                   std::size_t const* ring_ends,
                   std::size_t num_rings,
                   std::int32_t min_x,
                   std::int32_t min_y,
                   std::int32_t max_x,
                   std::int32_t max_y,
                   int fill_type,
                   void* context,
                   gdf_wagyu_point_fn on_point,
                   gdf_wagyu_end_fn on_ring_end,
                   gdf_wagyu_end_fn on_polygon_end,
                   char* error,
                   std::size_t error_len) {
    try {
        box const bounds(point(min_x, min_y), point(max_x, max_y));
        mapbox::geometry::wagyu::wagyu<coordinate> clipper;
        std::size_t start = 0;
        for (std::size_t r = 0; r < num_rings; ++r) {
            linear_ring ring;
            for (std::size_t i = start; i < ring_ends[r]; ++i) {
                ring.push_back(point(coords[2 * i], coords[2 * i + 1]));
            }
            start = ring_ends[r];
            auto clipped = mapbox::geometry::wagyu::quick_clip::quick_lr_clip(ring, bounds);
            if (!clipped.empty()) {
                clipper.add_ring(clipped, mapbox::geometry::wagyu::polygon_type_subject);
            }
        }
        multi_polygon result;
        clipper.execute(mapbox::geometry::wagyu::clip_type_union, result,
                        static_cast<mapbox::geometry::wagyu::fill_type>(fill_type),
                        mapbox::geometry::wagyu::fill_type_even_odd);
        for (auto const& polygon : result) {
            for (auto const& ring : polygon) {
                for (auto const& p : ring) {
                    on_point(context, p.x, p.y);
                }
                on_ring_end(context);
            }
            on_polygon_end(context);
        }
        return 0;
    } catch (std::exception const& e) {
        copy_message(e.what(), error, error_len);
        return 1;
    } catch (...) {
        copy_message("unknown error", error, error_len);
        return 1;
    }
}

} // extern "C"
