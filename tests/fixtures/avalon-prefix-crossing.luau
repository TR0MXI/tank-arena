-- Fixed synthetic observations; the fixture world supplies geometry and speeds, not future actions.
local Frame = require('./model/frame')
local Tracks = require('./perception/tracks')
local Risk = require('./safety/risk')
local evade = require('./safety/evasion')
local Safety = require('./safety/maneuver')

return function(source)
    local o = table.clone(source)
    o.maze = string.rep(string.rep(' ', 33) .. '\n', 32) .. string.rep(' ', 33)
    local me = {slot_id=1, team_id=0, alive=true, x=4.5*o.cell, y=4.5*o.cell, heading=0}
    local enemy = {slot_id=2, team_id=1, alive=true, x=10*o.cell, y=me.y, heading=128}
    o.me, o.tanks, o.shells = me, {me, enemy}, {}
    o.tick, o.ammo, o.drive, o.turn = 150, 4, 'forward', 'none'
    Frame.update(o)
    Tracks.update(o, o.tick)

    -- [offset in logical pixels (200 per cell), tick when the shell crosses our starting row].
    for _, trial in {{250,48}, {220,42}, {190,36}, {115,18}, {90,12}, {30,6}} do
        o.tick += 150
        Frame.update(o)
        Tracks.update(o, o.tick)
        evade(o, nil, enemy, 'forward', 'none', 0, 0) -- clear the previous evasion commitment
        local sx = me.x + o.cell * trial[1] / 200
        local sy = me.y - o.shell_speed * trial[2]
        local shell = {base=0, x0=sx, y0=sy, armedAt=1}
        for k = 1, 20 do
            shell[2*k-1], shell[2*k] = sx, sy + o.shell_speed*3*k
        end
        local paths = {shell, steps=0, memo={}, blockers={}, detailed=false}
        Risk.prepareRisk(o, me, paths)
        local cost, _, _, _, _, hit = Risk.score(o, me, paths, 'forward', 'none')
        local brake = math.clamp(math.floor(hit/6), 3, 8)
        local after = Risk.score(o, me, paths, 'forward', 'none', nil, brake)
        local d, r = evade(o, paths, enemy, 'forward', 'none', 0, 0)
        local dd, rr = Safety.nearEscape(o, me, paths, d, r)
        assert(cost >= 500000 and dd == d and rr == r, 'coarse and per-tick checks should agree here')
        if trial[2] >= 36 then
            assert(hit >= 24 and hit <= 60)
            assert(after <= 850 and d == 'forward' and r == 'none', 'safe continuation should keep the approach')
        else
            assert(hit < 24 and (d ~= 'forward' or r ~= 'none'), 'an immediate hazard must not be waived')
        end
    end
    return {fire=true}
end
