--!strict
local Fire = require("./fire")
local Frame = require("../model/frame")
local Geo = require("../model/geometry")
local Nav = require("./navigation")
local Tracks = require("../perception/tracks")
local Tuning = require("../model/tuning")
local Types = require("../model/types")

type Guard = Types.Guard
type Obs = Types.Obs
type Tank = Types.Tank

local flank: number?, flankFrom: number?, flankUntil, blockedSince: number? = nil, nil, 0, nil
local function targetChanged()
    if flank then
        Nav.reset()
    end
    flank, flankFrom, blockedSince = nil, nil, nil
end

local flankScores: { [number]: number } = {}
local function selectFlank(o: Obs, me: Tank, target: Tank, guards: { Guard }?): number?
    local here = Geo.cell(me.x, me.y)
    local tx, ty = Tracks.predict(target, Tuning.HORIZON_TICKS)
    local dx, dy = tx - me.x, ty - me.y
    local len = math.max(1, math.sqrt(dx * dx + dy * dy))
    local best: number?, score = nil, -math.huge
    local seed = o.behavior_seed
    local vary = seed ~= nil and Tuning.ROUTE_TEMP > 0 and not guards and Nav.quietChoice(o, me)
    if vary then
        table.clear(flankScores)
    end
    for d = 1, 4 do
        local v = Geo.neighbor(here, d)
        if not v then
            continue
        end
        local x, y = Geo.center(v)
        local value = math.abs((x - me.x) * dy - (y - me.y) * dx) / len
        if Geo.direct(x, y, tx, ty, Frame.shell) then
            value += Frame.cell
        end
        local range = math.sqrt((x - tx) ^ 2 + (y - ty) ^ 2)
        value += math.clamp(len - range, -Frame.cell, Frame.cell) * 0.7
        value -= math.abs(Geo.errorTo(me, x, y)) * 2
        if not Frame.solo then
            for _, e in o.tanks do
                if e.alive and e.team_id ~= me.team_id and e.slot_id ~= target.slot_id and Tracks.armed(e) then
                    local d2 = (x - e.x) ^ 2 + (y - e.y) ^ 2
                    if
                        d2 < (2.5 * Frame.cell) ^ 2
                        and math.abs(Geo.errorTo(e, x, y)) < 46
                        and Geo.direct(e.x, e.y, x, y, Frame.shell)
                    then
                        value -= Frame.cell * 0.85
                    end
                end
            end
        end
        if guards then
            for _, g in guards do
                local gx, gy = Tracks.predict(g.t, Tuning.HORIZON_TICKS)
                local gap = math.sqrt((x - gx) ^ 2 + (y - gy) ^ 2)
                value -= 2 * math.max(0, g.range - g.room - gap)
            end
        end
        if v == flankFrom or range < 0.7 * Frame.cell then
            value -= Frame.cell
        end
        for _, a in o.tanks do
            if a.alive and a.slot_id ~= me.slot_id and a.team_id == me.team_id then
                -- 优先从不同通道接近目标；各机器人的虚拟机之间不共享格子预订信息。
                local ax, ay = Tracks.predict(a, Tuning.HORIZON_TICKS)
                local gap = math.sqrt((ax - x) ^ 2 + (ay - y) ^ 2)
                value -= math.max(0, 1.8 * Frame.cell - gap)
                if Tracks.bySlot[target.slot_id].covered then
                    local ux, uy = x - tx, y - ty
                    local vx, vy = ax - tx, ay - ty
                    local alignment = (ux * vx + uy * vy) / math.max(1, range * math.sqrt(vx * vx + vy * vy))
                    value -= math.max(0, alignment - 0.4) * Frame.cell
                end
                if Geo.cell(a.x, a.y) == v then
                    value -= 3 * Frame.cell
                end
            end
        end
        if vary and v ~= flankFrom and range >= 0.7 * Frame.cell then
            flankScores[v] = value
        end
        if value > score then
            best, score = v, value
        end
    end
    if vary and seed and best then
        local goal = Geo.cell(tx, ty)
        local width = 420 * Tuning.ROUTE_TEMP
        local original = best
        local bx, by = Geo.center(original)
        local rank = score + width * Nav.preference(seed, here, goal, original)
        for d = 1, 4 do
            local v = Geo.neighbor(here, d)
            local value = if v then flankScores[v] else nil
            if v and v ~= original and value and value > score - width then
                local candidate = value + width * Nav.preference(seed, here, goal, v)
                if candidate > rank and Nav.openAlternative(o, me, v) then
                    local x, y = Geo.center(v)
                    if
                        Geo.direct(x, y, tx, ty, Frame.shell)
                        or not Geo.direct(bx, by, tx, ty, Frame.shell)
                    then
                        best, rank = v, candidate
                    end
                end
            end
        end
    end
    return best
end

@native
local function attack(
    o: Obs,
    me: Tank,
    t: Tank,
    aimX: number?,
    aimY: number?,
    ally: boolean,
    now: number,
    shooting: boolean,
    guards: { Guard }?
): (string, string)
    local x, y = Fire.firingSolution(o, me, t, aimX, aimY)
    if flank then
        local fx, fy = Geo.center(flank)
        if (shooting and not ally) or now >= flankUntil or (me.x - fx) ^ 2 + (me.y - fy) ^ 2 < 96 ^ 2 then
            flank = nil
            Nav.reset()
        end
    end
    if not flank and x and y then
        local dx, dy = t.x - me.x, t.y - me.y
        local range = math.sqrt(dx * dx + dy * dy)
        local err = Geo.errorTo(me, x, y)
        -- 松扳机不等于缺弹；没有好炮线时，选一个侧切目标并走完。
        if shooting then
            blockedSince = nil
        elseif now >= Fire.readyAt(t.slot_id) and o.can_fire and math.abs(err) < 9 then
            blockedSince = blockedSince or now
        else
            blockedSince = nil
        end
        -- 敌车正被队友架住时，利用补射间隔换线；无人掩护就不要提前丢掉炮线。
        local peelAfterShot = Tracks.bySlot[t.slot_id].covered
            and guards
            and not shooting
            and now < Fire.readyAt(t.slot_id)
        -- 即将归还的弹槽足够在转正后开火；不要在窗口前一刻把整条进攻路线扔掉。
        local dry = o.ammo == 0 and Tracks.bySlot[me.slot_id].refill > math.max(10, math.abs(err) / Frame.spin)
        local crossfire = guards and (#guards >= 2 or guards[1].t.slot_id ~= t.slot_id)
        if
            ally
            or dry
            or peelAfterShot
            or (crossfire and not shooting and o.tick < Fire.readyAt(t.slot_id))
            or (blockedSince and now - blockedSince > 25)
        then
            flank = selectFlank(o, me, t, guards)
            flankFrom = Geo.cell(me.x, me.y)
            flankUntil, blockedSince = now + 68, nil
            Nav.reset()
        else
            local drive = "stop"
            -- 已切入直射线时保持前压；该距离仅决定进攻油门，后续留距、友车和实弹检查仍能接管。
            if range > (if o.drive == "forward" then 0.48 else 0.70) * Frame.cell then
                -- 看转弯中段而非只看当前朝向。大角度对炮时也可以沿弧接近；没有畅通弧线再停车。
                local bend = math.clamp(err, -Frame.spin * 5, Frame.spin * 5)
                local advance = math.cos((err - bend) * math.pi / 128)
                local d = if advance > (if o.drive == "forward" then -0.15 else 0.12)
                    then "forward"
                    elseif advance < -0.42 then "reverse"
                    else "stop"
                if d ~= "stop" then
                    local reach = if d == "forward" then 320 else -240
                    local hx, hy = Geo.vector(me.heading + bend, reach)
                    if Nav.laneClear(o, me, me.x + hx, me.y + hy, Frame.hull) then
                        drive = d
                    end
                end
            end
            return drive, Nav.trim(o, err, false)
        end
    else
        blockedSince = nil
    end
    return Nav.navigate(o, me, flank or Geo.cell(t.x, t.y), now)
end

-- 响应真实位移，而不是把「有按键」当作「已走动」。长炮管贴墙时会连原地转身也被裁判挡住。
local anchorX: number?, anchorY, anchorAt, freeDrive, freeUntil = nil, 0, 0, "stop", 0
local function freeRun(o: Obs, me: Tank, sign: number)
    local ux, uy = Geo.vector(me.heading, 1)
    local dx, dy = ux * sign * 0.5 * Frame.cell, uy * sign * 0.5 * Frame.cell
    local free = 1
    -- 平移时只需扫前缘（倒车时为后缘）；再检查附近车体，别解围时撞上队友。
    local nose = sign * o.hull_half_length
    for side = -1, 1 do
        local along = if sign == 1 and side == 0 then o.turret_offset + o.turret_half_length else nose
        local x, y = me.x + ux * along - uy * side * Frame.hull, me.y + uy * along + ux * side * Frame.hull
        free = math.min(free, Geo.cast(x, y, dx, dy, 0) or 1)
    end
    for _, t in o.tanks do
        if t.alive and t.slot_id ~= me.slot_id then
            local rx, ry = t.x - me.x, t.y - me.y
            if rx * dx + ry * dy > 0 then
                free = math.min(
                    free,
                    Geo.slab(me.x, me.y, dx, dy, t.x - 2 * Frame.hull, t.y - 2 * Frame.hull, t.x + 2 * Frame.hull, t.y + 2 * Frame.hull)
                        or 1
                )
            end
        end
    end
    return free
end

local function unstick(o: Obs, me: Tank, now: number, drive: string, turn: string): (string, string)
    if now < freeUntil then
        return freeDrive, "none"
    end
    if
        not anchorX
        or (o.drive == "stop" and o.turn == "none")
        or (me.x - anchorX) ^ 2 + (me.y - anchorY) ^ 2 > (0.13 * Frame.cell) ^ 2
    then
        anchorX, anchorY, anchorAt = me.x, me.y, now
    elseif now - anchorAt >= 28 and (drive ~= "stop" or turn ~= "none" or blockedSince) then
        local front, back = freeRun(o, me, 1), freeRun(o, me, -1)
        freeDrive = if front > back or (front == back and o.drive == "reverse") then "forward" else "reverse"
        freeUntil = now + 14
        anchorX, flank, blockedSince = nil, nil, nil
        Nav.reset()
        return freeDrive, "none"
    end
    return drive, turn
end

local function recovering(now: number): boolean
    return now < freeUntil
end

return {
    attack = attack,
    unstick = unstick,
    targetChanged = targetChanged,
    recovering = recovering,
}
