--!strict
local Frame = require("../model/frame")
local Geo = require("../model/geometry")
local Tuning = require("../model/tuning")
local Types = require("../model/types")

type Obs = Types.Obs
type Tank = Types.Tank

-- 车头转不动时，找能真正转舵的墙端，而不是按住转键等。找到可转舵的位置即停止，否则看到时域末。
local CORNER_TAIL = 3
local lastHeading: number?, pinnedSince: number = nil, 0
local fixedX, fixedY, stillSince = 0, 0, 0
local exitUntil, exitDrive, exitTurn = 0, "stop", "none"
@native
local function unlockDistance(
    o: Obs,
    me: Tank,
    speed: number,
    c: number,
    s: number,
    rc: number,
    rs: number
): number
    for k = 1, Tuning.HORIZON_TICKS do
        local x, y = me.x + c * speed * k, me.y + s * speed * k
        if not Geo.fits(o, x, y, c, s) then
            break
        end
        if Geo.fits(o, x, y, rc, rs) then
            -- 车身会沿这一条直线扫过；不能借用敌我车当前占着的通道。
            for _, a in o.tanks do
                if a.alive and a.slot_id ~= me.slot_id then
                    local dx, dy = a.x - me.x, a.y - me.y
                    local along = (dx * c + dy * s) * math.sign(speed)
                    if
                        along >= 0
                        and along < math.abs(speed) * k + o.hull_half_length + Frame.hull
                        and math.abs(dy * c - dx * s) < 2 * Frame.hull
                    then
                        return math.huge
                    end
                end
            end
            return k
        end
    end
    return math.huge
end

@native
local function cornerExit(o: Obs, me: Tank, drive: string, turn: string): string
    if fixedX ~= me.x or fixedY ~= me.y then
        fixedX, fixedY, stillSince = me.x, me.y, o.tick
    end
    if lastHeading ~= me.heading or o.turn == "none" then
        pinnedSince = o.tick
    end
    lastHeading = me.heading
    -- 内战中两根轴都被挡住时，非零油门不是前进的证据；勿干预仍在有效移动的进攻。
    local jammed = o.drive ~= "stop" and o.tick - stillSince >= 6
    if turn == "none" or (drive ~= "stop" and not jammed) or o.turn ~= turn then
        exitUntil = 0
        return drive
    end
    if o.tick < exitUntil and exitTurn == turn and o.drive == exitDrive and not jammed then
        return exitDrive
    end
    if o.tick - pinnedSince < 3 then
        return drive
    end
    local rc, rs = Geo.vector(me.heading + (if turn == "right" then Frame.spin else -Frame.spin), 1)
    if Geo.fits(o, me.x, me.y, rc, rs) then
        return drive
    end
    local c, s = Geo.vector(me.heading, 1)
    local front = if jammed and o.drive == "forward"
        then math.huge
        else unlockDistance(o, me, o.forward_speed, c, s, rc, rs)
    local back = if jammed and o.drive == "reverse"
        then math.huge
        else unlockDistance(o, me, -Frame.reverse, c, s, rc, rs)
    if math.min(front, back) == math.huge then
        return drive
    end
    exitDrive = if front <= back + 3 then "forward" else "reverse"
    exitTurn = turn
    exitUntil = o.tick + math.min(Tuning.COMMIT, if exitDrive == "forward" then front else back) + CORNER_TAIL
    return exitDrive
end

local function pinnedAt(): number
    return pinnedSince
end

local function stillAt(): number
    return stillSince
end

return {
    adjust = cornerExit,
    pinnedAt = pinnedAt,
    stillAt = stillAt,
}
