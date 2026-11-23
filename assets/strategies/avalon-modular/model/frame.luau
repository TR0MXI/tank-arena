--!strict
local Types = require("./types")
local frame = {
    cell = 0, hull = 0, spin = 0, reverse = 0, wall = 0, shell = 0, capacity = 0, muzzle = 0,
    pairReach = 0, cadence = 1, solo = false, width = 0, height = 0, lines = {} :: {string},
}
local previousTick: number?

function frame.update(o: Types.Obs)
    if previousTick == nil then
        frame.cell, frame.wall, frame.hull, frame.shell = o.cell, o.wall_half, o.hull_half_width, o.shell_radius
        frame.reverse, frame.spin, frame.capacity, frame.muzzle = o.reverse_speed, o.turn_step, o.ammo_limit, o.muzzle
        frame.pairReach = 2 * math.max(
            math.sqrt(o.hull_half_length ^ 2 + frame.hull ^ 2),
            math.sqrt((o.turret_offset + o.turret_half_length) ^ 2 + o.turret_half_width ^ 2)
        )
        frame.solo = #o.tanks == 2
        frame.lines = string.split(o.maze, "\n")
        frame.width, frame.height = math.floor(#frame.lines[1] / 2), math.floor(#frame.lines / 2)
    end
    frame.cadence = if previousTick then math.clamp(o.tick - previousTick, 1, 3) else 1
    previousTick = o.tick
end

return frame
