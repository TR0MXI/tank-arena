--!strict
local Corner = require("./corner")
local Fire = require("../tactics/fire")
local Frame = require("../model/frame")
local Geo = require("../model/geometry")
local Nav = require("../tactics/navigation")
local Risk = require("./risk")
local Tracks = require("../perception/tracks")
local Tuning = require("../model/tuning")
local Types = require("../model/types")

type Guard = Types.Guard
type Obs = Types.Obs
type Obstacle = Types.Obstacle
type Path = Types.Path
type Paths = Types.Paths
type Tank = Types.Tank

-- 打过这一炮且双方仍在对线上时，不站定等回弹：倒车带弧，优先弯向通的那一侧。
local arcSlot: number?, arcTurn: string, arcUntil = nil, "none", 0
local function peel(o: Obs, me: Tank, g: Guard, d: string)
    if arcSlot ~= g.t.slot_id or o.tick >= arcUntil then
        local sign = if d == "reverse" then -1 else 1
        local lx, ly = Geo.vector(me.heading - 32, sign * 0.65 * Frame.cell)
        local rx, ry = Geo.vector(me.heading + 32, sign * 0.65 * Frame.cell)
        local left, right =
            Nav.laneClear(o, me, me.x + lx, me.y + ly, Frame.hull), Nav.laneClear(o, me, me.x + rx, me.y + ry, Frame.hull)
        if not left and not right then
            return "none"
        end
        local bias = Geo.errorTo(me, g.t.x, g.t.y)
        arcTurn = if left ~= right
            then (if left then "left" else "right")
            elseif bias > 1 or (bias >= -1 and me.slot_id % 2 == 0) then "left"
            else "right"
        arcSlot, arcUntil = g.t.slot_id, o.tick + 14
    end
    return arcTurn
end

local spacingSlot: number?
@native
local function keepRoom(
    o: Obs,
    me: Tank,
    drive: string,
    turn: string,
    guards: { Guard }?,
    g: Guard?
): (string, string)
    -- 进入和退出用不同阈值；在缓冲带内保留横切，不因相差一帧反复前后换挡。
    if not guards or not g or g.room > (if spacingSlot == g.t.slot_id then 0.34 else 0.15) * Frame.cell then
        spacingSlot = nil
        return drive, turn
    end
    spacingSlot = g.t.slot_id
    local h = me.heading + (if turn == "left" then -1 elseif turn == "right" then 1 else 0) * Frame.spin * Frame.cadence
    local hx, hy = Geo.vector(h, 1)
    local radial = (hx * (g.t.x - me.x) + hy * (g.t.y - me.y)) / g.range
    local signed = if drive == "forward" then radial elseif drive == "reverse" then -radial else 0
    if signed < -0.28 or (math.abs(radial) < 0.28 and drive ~= "stop") then
        local step = if drive == "forward" then Tuning.HORIZON_TICKS * o.forward_speed else -Tuning.HORIZON_TICKS * Frame.reverse
        if #guards == 1 or Risk.lostRoom(guards, me.x + hx * step, me.y + hy * step, Tuning.HORIZON_TICKS) < 16 then
            return drive, turn
        end
    end
    local reloading = o.ammo == 0 or o.tick < Fire.readyAt(g.t.slot_id)
    if g.room >= 0 and not reloading then
        return "stop", turn
    end
    local d = if radial >= 0 then "reverse" else "forward"
    local step = if d == "reverse" then -Frame.reverse * Tuning.HORIZON_TICKS else o.forward_speed * Tuning.HORIZON_TICKS
    local x, y = me.x + hx * step, me.y + hy * step
    if Risk.lostRoom(guards, x, y, Tuning.HORIZON_TICKS) < 16 and Nav.laneClear(o, me, x, y, Frame.hull) then
        if reloading and math.abs(radial) > 0.7 then
            turn = peel(o, me, g, d)
        end
        return d, turn
    end
    -- 退路是墙、友车或第二个近敌时，不能无限撞上去；先转出侧向空间。
    if turn == "none" then
        local side = Geo.errorTo(me, g.t.x, g.t.y)
        turn = if side > 0 or (side == 0 and me.slot_id % 2 == 0) then "left" else "right"
    end
    return "stop", turn
end

local drives = { "forward", "reverse", "stop" }
local turns = { "none", "left", "right" }
-- 不主动打破原地避弹/转正。只有已保持油门却不动时，验证一次可执行的短弧；
-- 离开墙端的头几步仍需逐次复核来弹，而不是盲目延长解困按键。
local observedDrive, observedTurn, controlSince = "stop", "none", 0
local breakUntil, breakD, breakT = -1, "stop", "none"
local breakDrives = { "forward", "reverse" }
local BREAK_MARGIN = 8000
@native
local function openingArc(o: Obs, me: Tank, d: string, r: string): (number, number, number)
    local x, y = me.x, me.y
    local c, s = Geo.vector(me.heading, 1)
    local dc, ds = Geo.vector(if r == "right" then Frame.spin elseif r == "left" then -Frame.spin else 0, 1)
    local speed = if d == "forward" then o.forward_speed else -Frame.reverse
    local length = 0
    for _ = 1, Tuning.HORIZON_TICKS do
        local nc, ns = c * dc - s * ds, s * dc + c * ds
        if r ~= "none" and Geo.fits(o, x, y, nc, ns) then
            c, s = nc, ns
        end
        local nx, ny = x + c * speed, y + s * speed
        if Geo.fits(o, nx, ny, c, s) then
            length += math.abs(speed)
            x, y = nx, ny
        end
    end
    return length, x, y
end

@native
local function breakContact(
    o: Obs,
    me: Tank,
    paths: Paths?,
    target: Tank?,
    d: string,
    r: string
): (string, string)
    if o.drive ~= observedDrive or o.turn ~= observedTurn then
        observedDrive, observedTurn, controlSince = o.drive, o.turn, o.tick
    end
    if not paths or d == "stop" or #paths > 8 then
        breakUntil = -1
        return d, r
    end
    local holding = o.tick < breakUntil and o.drive == breakD and o.turn == breakT
    if not holding then
        breakUntil = -1
    end
    if
        not holding
        and (
            o.drive == "stop"
            or o.turn == "none"
            or o.tick - math.max(controlSince, Corner.stillAt(), Corner.pinnedAt()) < 8
        )
    then
        return d, r
    end
    -- 邻车也会同时移动；只检查墙面的试探不能用来挤进另一辆车的碰撞体。
    for _, t in o.tanks do
        if t.alive and t.slot_id ~= me.slot_id and (t.x - me.x) ^ 2 + (t.y - me.y) ^ 2 < (1.8 * Frame.cell) ^ 2 then
            breakUntil = -1
            return d, r
        end
    end
    local nominal = Risk.score(o, me, paths, d, r)
    if holding then
        -- 动作尚在生效也要重新过避弹评分；预测撞弹（50 万以上）绝不接受。
        local cost = Risk.score(o, me, paths, breakD, breakT)
        if cost < 500000 and cost <= nominal + BREAK_MARGIN then
            return breakD, breakT
        end
        breakUntil = -1
    end
    if openingArc(o, me, d, r) >= 2 * o.forward_speed then
        return d, r
    end
    local best, bestD, bestT = math.huge, d, r
    local tx, ty = me.x, me.y
    if target then
        tx, ty = Tracks.predict(target, Tuning.HORIZON_TICKS)
    end
    local range = math.sqrt((tx - me.x) ^ 2 + (ty - me.y) ^ 2)
    for _, candidate in breakDrives do
        for _, turn in turns do
            local travel, x, y = openingArc(o, me, candidate, turn)
            if travel < 2 * Frame.reverse then
                continue
            end
            local cost = Risk.score(o, me, paths, candidate, turn)
            if cost >= 500000 or cost > nominal + BREAK_MARGIN then
                continue
            end
            local score = cost - travel * 10
            if target then
                score += (math.sqrt((tx - x) ^ 2 + (ty - y) ^ 2) - range) * 3
            end
            if score < best then
                best, bestD, bestT = score, candidate, turn
            end
        end
    end
    if best < math.huge then
        breakUntil, breakD, breakT = o.tick + 10, bestD, bestT
    end
    return bestD, bestT
end

-- BEGIN 近弹复核：只用当前公开弹路，不推测下一发。每步先转车、再移动、最后验弹。
local REFLEX_TICKS = Tuning.HORIZON_TICKS
@native
local function crosses(ax: number, ay: number, bx: number, by: number, h: number, w: number): boolean
    -- 线段和车体局部坐标矩形的三个分离轴。半径扩张在矩形圆角处略保守。
    return math.abs(ax + bx) <= 2 * h + math.abs(bx - ax)
        and math.abs(ay + by) <= 2 * w + math.abs(by - ay)
        and math.abs(ax * by - ay * bx) <= h * math.abs(by - ay) + w * math.abs(bx - ax)
end

@native
local function clearsNear(
    o: Obs,
    me: Tank,
    paths: { Path },
    cars: { Obstacle },
    d: string,
    r: string,
    release: number?
): (boolean, number)
    local x, y = me.x, me.y
    local c, s = Geo.vector(me.heading, 1)
    local speed = if d == "forward" then o.forward_speed elseif d == "reverse" then -Frame.reverse else 0
    local dc, ds = Geo.vector(if r == "left" then -Frame.spin elseif r == "right" then Frame.spin else 0, 1)
    for step = 1, REFLEX_TICKS do
        local pc, ps = c, s
        if r ~= "none" and (not release or step <= release) then
            local nc, ns = c * dc - s * ds, s * dc + c * ds
            if Geo.fits(o, x, y, nc, ns) then
                c, s = nc, ns
            end
        end
        local nx, ny = x + c * speed, y + s * speed
        if speed ~= 0 and not Geo.fits(o, nx, ny, c, s) then
            nx, ny = x, y
        end
        for _, t in cars do
            if Geo.carOverlap(o, nx, ny, c, s, t, math.min(step - 1, t.coast)) then
                nx, ny, c, s = x, y, pc, ps
                break
            end
        end
        x, y = nx, ny
        local slice = math.ceil(step / Tuning.SLICE)
        local phase = (step - 1) % Tuning.SLICE
        for _, p in paths do
            local j = p.base + slice
            if j < p.armedAt or not p[2 * j] then
                continue
            end
            local ax = (if j == 1 then p.x0 else p[2 * j - 3]) - x
            local ay = (if j == 1 then p.y0 else p[2 * j - 2]) - y
            local dx, dy = (p[2 * j - 1] - x - ax) / Tuning.SLICE, (p[2 * j] - y - ay) / Tuning.SLICE
            ax, ay = ax + phase * dx, ay + phase * dy
            if math.max(math.abs(ax), math.abs(ay)) > Frame.pairReach + o.shell_speed then
                continue
            end
            local a, b = ax * c + ay * s, ay * c - ax * s
            local e, f = a + dx * c + dy * s, b + dy * c - dx * s
            if
                crosses(a, b, e, f, o.hull_half_length + Frame.shell, Frame.hull + Frame.shell)
                or crosses(
                    a - o.turret_offset,
                    b,
                    e - o.turret_offset,
                    f,
                    o.turret_half_length + Frame.shell,
                    o.turret_half_width + Frame.shell
                )
            then
                return false, step
            end
        end
    end
    return true, REFLEX_TICKS
end

@native
local function nearEscape(o: Obs, me: Tank, paths: Paths?, d: string, r: string): (string, string)
    if not paths or #paths > 4 or #o.shells > 12 then
        return d, r
    end
    local near = {}
    local radius = Frame.pairReach / 2 + Frame.shell + REFLEX_TICKS * (o.shell_speed + o.forward_speed)
    for i = 1, #paths do
        local p = paths[i]
        local j = p.base
        local ax, ay = if j == 0 then p.x0 else p[2 * j - 1], if j == 0 then p.y0 else p[2 * j]
        if
            not p.guess
            and p.armedAt <= j + math.ceil(REFLEX_TICKS / Tuning.SLICE)
            and (ax - me.x) ^ 2 + (ay - me.y) ^ 2 <= radius ^ 2
        then
            table.insert(near, p)
        end
    end
    if #near == 0 then
        return d, r
    end
    local cars = {}
    for _, t in o.tanks do
        if
            t.alive
            and t.slot_id ~= me.slot_id
            and (t.x - me.x) ^ 2 + (t.y - me.y) ^ 2 <= (Frame.pairReach + 2 * REFLEX_TICKS * o.forward_speed) ^ 2
        then
            local c, s = Geo.vector(t.heading, 1)
            local m = Tracks.bySlot[t.slot_id]
            table.insert(
                cars,
                {
                    x = t.x,
                    y = t.y,
                    c = c,
                    s = s,
                    vx = m.vx,
                    vy = m.vy,
                    heading = t.heading,
                    coast = Tracks.coastTime(t, m),
                }
            )
        end
    end
    local clear, hit = clearsNear(o, me, near, cars, d, r)
    if not clear and hit >= 9 and r ~= "none" and d ~= "stop" then
        local bend = math.clamp(math.floor(hit / (2 * Tuning.SLICE)) + 1, 3, 8)
        if Risk.score(o, me, paths, d, r, bend) < 500000 then
            clear = clearsNear(o, me, near, cars, d, r, (bend-1) * Tuning.SLICE)
        end
    end
    if clear then
        return d, r
    end
    local best, bd, br = math.huge, d, r
    for _, nd in drives do
        for _, nr in turns do
            if (nd ~= d or nr ~= r) and clearsNear(o, me, near, cars, nd, nr) then
                -- 逐步复核通过再比较同一时域的风险分；同分先少换一根轴。
                local score = Risk.score(o, me, paths, nd, nr)
                    + (if nd == d then 0 else 2)
                    + (if nr == r then 0 else 1)
                if score < best then
                    best, bd, br = score, nd, nr
                end
            end
        end
    end
    return bd, br
end
-- END 近弹复核

local function spacing(): boolean
    return spacingSlot ~= nil
end

return {
    breakContact = breakContact,
    drives = drives,
    keepRoom = keepRoom,
    nearEscape = nearEscape,
    spacing = spacing,
    turns = turns,
}
