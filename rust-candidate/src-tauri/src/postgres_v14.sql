-- Derived from original CS Demo Manager v14.ts Kysely source operations and fixed upstream data.
-- Original MIT source retained; caller owns transaction and marker.

update "maps" set "position_x" = -2000, "position_y" = 3250, "scale" = 5.5 where "name" = 'de_cache' and "game" = 'CS2';
