--!strict
local Corner = require("./corner")
local Frame = require("../model/frame")
local Geo = require("../model/geometry")
local Tracks = require("../perception/tracks")
local Tuning = require("../model/tuning")
local Types = require("../model/types")

type Guard = Types.Guard
type Obs = Types.Obs
type Obstacle = Types.Obstacle
type Path = Types.Path
type Paths = Types.Paths
type Shot = Types.Shot
type Tank = Types.Tank

local lookSamples = Tuning.SAMPLES
local clearance = Tuning.BUFFER
-- 局部避弹：把附近所有弹（包括己方和自己的反弹）投影到短时间轴上一次，
-- 比较原计划及前进/倒车/转向的替代轨迹。先检验原计划，安全时不抢控制权。
type Trace = {
    id: number,
    at: number,
    vx: number,
    vy: number,
    expiry: number,
    armed: boolean,
    firstBounce: number,
    path: Path,
}
local traces: { Trace } = {}
local REUSE = 5
@native
local function traceShot(o: Obs, s: Shot): Trace
    local x, y, vx, vy = s.x, s.y, s.vx, s.vy
    -- 未碰墙的己弹不能击中发射者；仍预测其首跳，避免把回弹一起漏掉。
    local path: Path = {
        base = 0,
        x0 = x,
        y0 = y,
        armedAt = if s.owner ~= o.slot_id or s.deadly_to_owner then 1 else math.huge,
    }
    -- 一次找出这条直线的下一次撞墙时刻；采样经过墙面才重算，而非每个采样点重复扫墙。
    local horizon = math.min(s.life, Tuning.SLICE * (lookSamples + REUSE))
    local firstBounce = math.huge
    local time, bounceAt: number?, flipX, flipY = 0, nil, 0, 0
    for k = 1, math.ceil(horizon / Tuning.SLICE) do
        local untilTime = math.min(k * Tuning.SLICE, horizon)
        if untilTime <= time then
            break
        end
        for _ = 1, 4 do
            if time >= untilTime then
                break
            end
            if not bounceAt then
                local left = horizon - time
                local hit, fx, fy = Geo.cast(x, y, vx * left, vy * left, Frame.shell, true)
                bounceAt = if hit then time + hit * left else math.huge
                flipX, flipY = fx, fy
            end
            local nextTime = math.min(untilTime, bounceAt :: number)
            local dt = nextTime - time
            x, y, time = x + vx * dt, y + vy * dt, nextTime
            if time == bounceAt then
                firstBounce = math.min(firstBounce, time)
                path.armedAt = math.min(path.armedAt, k)
                local dot = 2 * (vx * flipX + vy * flipY)
                vx, vy = vx - dot * flipX, vy - dot * flipY
                -- 推出浮点接触面，避免在同一墙端重复得到零飞行时间。
                x, y = x + math.sign(vx) * 0.0001, y + math.sign(vy) * 0.0001
                bounceAt = nil
            end
        end
        -- 极端墙角保守地停在接触点；下一采样重新求交，不在这个面上死循环。
        if time < untilTime then
            time, bounceAt = untilTime, nil
        end
        path[2 * k - 1], path[2 * k] = x, y
    end
    return {
        id = s.id,
        at = o.tick,
        vx = s.vx,
        vy = s.vy,
        expiry = o.tick + s.life,
        armed = s.deadly_to_owner,
        firstBounce = firstBounce,
        path = path,
    }
end

@native
local function forecast(o: Obs, me: Tank): (Paths, boolean)
    local paths: Paths = { steps = 0, blockers = {}, memo = {}, detailed = false }
    local ownReturn = false
    local reach = (o.shell_speed + o.forward_speed) * Tuning.SLICE * lookSamples + o.hit_radius + 100
    -- 先按威胁排序附近炮弹，再推演可能的反弹路径。
    -- 即使八辆坦克各有五发炮弹在场，也不能超出单次决策配额。
    type Candidate = { shot: Shot, score: number }
    local nearby: { Candidate } = {}
    for _, s in o.shells do
        local dx, dy = s.x - me.x, s.y - me.y
        if dx * dx + dy * dy > reach * reach then
            continue
        end
        local v2 = s.vx * s.vx + s.vy * s.vy
        local t = math.clamp(-(dx * s.vx + dy * s.vy) / math.max(1, v2), 0, math.min(s.life, Tuning.SLICE * lookSamples))
        table.insert(nearby, { shot = s, score = (dx + t * s.vx) ^ 2 + (dy + t * s.vy) ^ 2 + 4 * t * t })
    end
    table.sort(nearby, function(a: Candidate, b: Candidate)
        return a.score < b.score or (a.score == b.score and a.shot.id < b.shot.id)
    end)
    for i = 1, math.min(#nearby, 12) do
        local s = nearby[i].shot
        -- 同一颗弹按观察步数模 SLICE 分相。只复用起始直线未碰墙、位置和速度仍吻合的预测；
        -- 跳帧、提前销弹、实际反弹和槽位冲突都不需要从旧观察推断。
        local key = (s.id % 32) * Tuning.SLICE + o.tick % Tuning.SLICE + 1
        local cached = traces[key]
        local elapsed = if cached then o.tick - cached.at else 0
        if
            not cached
            or cached.id ~= s.id
            or elapsed < 0
            or elapsed > Tuning.SLICE * REUSE
            or cached.expiry ~= o.tick + s.life
            or cached.armed ~= s.deadly_to_owner
            or elapsed >= cached.firstBounce
            or s.vx ~= cached.vx
            or s.vy ~= cached.vy
            or math.abs(s.x - cached.path.x0 - elapsed * s.vx) > 0.00001
            or math.abs(s.y - cached.path.y0 - elapsed * s.vy) > 0.00001
        then
            cached = traceShot(o, s)
            traces[key] = cached
            elapsed = 0
        end
        local path = cached.path
        local base = elapsed / Tuning.SLICE
        path.base = base
        local reachable = false
        local checkReturn = s.owner == me.slot_id and not ownReturn
        local radius = o.hit_radius + Tuning.SLICE * o.shell_speed / 2
        for j = math.max(base + 1, path.armedAt), math.min(base + lookSamples, #path / 2) do
            local gap = math.max(math.abs(path[2 * j - 1] - me.x), math.abs(path[2 * j] - me.y))
            -- 本车到采样末最多走这段距离；再加一整段弹程，不能漏掉两点之间的穿越。
            local bound = (j - base) * Tuning.SLICE * o.forward_speed + Tuning.SLICE * o.shell_speed + Frame.muzzle + clearance + 100
            reachable = reachable or gap <= bound
            if checkReturn and gap < radius then
                ownReturn = true
            end
            if reachable and (not checkReturn or ownReturn) then
                break
            end
        end
        if reachable then
            if s.owner == me.slot_id then
                paths.detailed = true
            end
            table.insert(paths, path)
        end
    end
    return paths, ownReturn
end

-- 有弹、且炮口已指向我方车身的近敌：前压时提前横切；虚拟首发不算真实命中，不预言反弹。
@native
local function prefire(o: Obs, me: Tank, t: Tank?): Path?
    if not t then
        return nil
    end
    local dx, dy = me.x - t.x, me.y - t.y
    local c, s = Geo.vector(t.heading, 1)
    local along, across = dx * c + dy * s, dy * c - dx * s
    if
        along < Frame.muzzle
        or along > Tuning.FEINT * Frame.cell
        or math.abs(across) > o.hit_radius
        or not Geo.direct(t.x, t.y, me.x, me.y, Frame.shell)
    then
        return nil
    end
    local x, y = t.x + Frame.muzzle * c, t.y + Frame.muzzle * s
    local p: Path = { base = 0, x0 = x, y0 = y, armedAt = 1, guess = true }
    local kmax = math.min(lookSamples, math.ceil((along - Frame.muzzle + o.hull_half_length) / (Tuning.SLICE * o.shell_speed)))
    local range = kmax * Tuning.SLICE * o.shell_speed
    kmax = math.floor(kmax * (Geo.cast(x, y, c * range, s * range, Frame.shell) or 1))
    for k = 1, kmax do
        p[2 * k - 1], p[2 * k] = x + c * k * Tuning.SLICE * o.shell_speed, y + s * k * Tuning.SLICE * o.shell_speed
    end
    return if kmax > 0 then p else nil
end

-- 所有密度和单挑都使用同一动作时域。实弹碰撞已确定时可提前终止。
local obstacleCache: { Obstacle } = {}
@native
local function prepareRisk(o: Obs, me: Tank, paths: Paths)
    local steps = lookSamples
    local reach = 2 * steps * Tuning.SLICE * o.forward_speed + Frame.pairReach
    local blockers = paths.blockers
    for _, t in o.tanks do
        if
            t.alive
            and t.slot_id ~= me.slot_id
            and math.max(math.abs(t.x - me.x), math.abs(t.y - me.y)) < reach
        then
            local b = obstacleCache[t.slot_id]
            if not b then
                b = { x = 0, y = 0, c = 0, s = 0, heading = -1, vx = 0, vy = 0, coast = 0 }
                obstacleCache[t.slot_id] = b
            end
            b.x, b.y = t.x, t.y
            if paths.detailed then
                if b.heading ~= t.heading then
                    b.heading = t.heading
                    b.c, b.s = Geo.vector(t.heading, 1)
                end
            end
            local m = Tracks.bySlot[t.slot_id]
            b.vx, b.vy, b.coast = m.vx, m.vy, Tracks.coastTime(t, m)
            table.insert(blockers, b)
        end
    end
    paths.steps = steps
end

-- 只惩罚进一步挤掉反应余量；原本就在圈内不会因此放弃能活的闪避路线。
@native
local function lostRoom(guards: { Guard }, x: number, y: number, ticks: number?)
    local loss = 0
    for _, g in guards do
        local tx, ty = g.t.x, g.t.y
        if ticks then
            tx, ty = Tracks.predict(g.t, ticks)
        end
        local after = math.sqrt((x - tx) ^ 2 + (y - ty) ^ 2)
        loss = math.max(loss, g.range - math.max(0, g.room) - after)
    end
    return loss
end

@native
local function rollout(
    o: Obs,
    me: Tank,
    paths: Paths,
    drive: string,
    turn: string,
    straighten: number?
): (number, number, number, number, number, number)
    local x, y = me.x, me.y
    local speed = if drive == "forward" then o.forward_speed elseif drive == "reverse" then -Frame.reverse else 0
    local spin = if turn == "left" then -Frame.spin elseif turn == "right" then Frame.spin else 0
    local cost = 0
    local ax, ay, ac, asine = 0, 0, 1, 0
    local c, s = Geo.vector(me.heading, 1)
    -- 每段使用相同的转角，方向用复数乘法旋转；直行、刹停都不用逐采样点算正余弦。
    local dc, ds = Geo.vector(spin * Tuning.SLICE, 1)
    local mc, ms = Geo.vector(spin * (Tuning.SLICE + 1) / 2, speed)
    local pinned = spin ~= 0 and turn == o.turn and o.tick - Corner.pinnedAt() >= 3
    local tc, ts = 1, 0
    if pinned then
        tc, ts = Geo.vector(spin, 1)
    end
    for k = 1, paths.steps do
        if k == straighten then
            spin, dc, ds, mc, ms = 0, 1, 0, speed, 0
        end
        local px, py, pc, ps = x, y, c, s
        local mx, my = c * mc - s * ms, s * mc + c * ms
        c, s = c * dc - s * ds, s * dc + c * ds
        local dx, dy = mx * Tuning.SLICE, my * Tuning.SLICE
        if pinned and spin ~= 0 and not Geo.fits(o, x, y, pc * tc - ps * ts, ps * tc + pc * ts) then
            -- 裁判先在原位尝试转舵、再平移。沿墙滑移可能走到能转舵的位置，下一小段重新判断。
            c, s = pc, ps
            mx, my = pc * speed, ps * speed
            dx, dy = mx * Tuning.SLICE, my * Tuning.SLICE
        else
            pinned = false
        end
        if (speed ~= 0 or spin ~= 0) and not Geo.fits(o, x + dx, y + dy, c, s) then
            if spin ~= 0 and not Geo.fits(o, x, y, c, s) then
                c, s = pc, ps
                mx, my = c * speed, s * speed
            end
            if speed ~= 0 and Geo.fits(o, x + mx, y + my, c, s) then
                dx, dy = mx, my
            else
                dx, dy = 0, 0
            end
        end
        if paths.detailed and (speed ~= 0 or spin ~= 0) then
            local blocked = false
            for _, t in paths.blockers do
                if Geo.carOverlap(o, x + dx, y + dy, c, s, t, math.min(t.coast, (k - 1) * Tuning.SLICE)) then
                    blocked = true
                    break
                end
            end
            if blocked then
                -- 与裁判一致：整车姿态遭阻，转角也不能凭空推进。
                c, s = pc, ps
            else
                x += dx
                y += dy
            end
        elseif speed ~= 0 then
            local blocked = false
            for _, t in paths.blockers do
                local dt = math.min(t.coast, (k - 1) * Tuning.SLICE)
                if math.max(math.abs(t.x + dt * t.vx - x - dx), math.abs(t.y + dt * t.vy - y - dy)) < 2 * Frame.hull then
                    blocked = true
                    break
                end
            end
            if not blocked then
                x += dx
                y += dy
            end
        end
        if k == paths.poseSlice then ax, ay, ac, asine = x, y, c, s end
        if paths.guards and k == 3 then
            cost += lostRoom(paths.guards, x, y, k * Tuning.SLICE) * 40
        end
        for i = 1, #paths do
            local p = paths[i]
            local j = p.base + k
            local sx, sy = p[2 * j - 1], p[2 * j]
            if not sx or j < p.armedAt then
                continue
            end
            local ax, ay =
                (if j == 1 then p.x0 else p[2 * j - 3]) - px, (if j == 1 then p.y0 else p[2 * j - 2]) - py
            local bx, by = sx - x, sy - y
            -- 最近点按相对运动取，不把两个采样点之间穿过车身的弹当成安全。
            local ux, uy = bx - ax, by - ay
            local len = ux * ux + uy * uy
            local t = if len == 0 then 0 else math.clamp(-(ax * ux + ay * uy) / len, 0, 1)
            local rx, ry = ax + t * ux, ay + t * uy
            if math.max(math.abs(rx), math.abs(ry)) > Frame.muzzle + clearance + 100 then
                continue
            end
            local fore, side = rx * c + ry * s, math.abs(ry * c - rx * s)
            local hullGap = math.max(math.abs(fore) - o.hull_half_length, side - Frame.hull)
            local turretGap =
                math.max(math.abs(fore - o.turret_offset) - o.turret_half_length, side - o.turret_half_width)
            local gap = math.min(hullGap, turretGap) - Frame.shell
            if gap < clearance then
                cost += (clearance - gap) * (lookSamples + 1 - k) * (if p.guess then 0.25 else 1)
            end
            if gap < 12 and not p.guess then
                return 500000 + (lookSamples - k) * 18000 + math.min(cost, 16000), x, y, c, s, k * Tuning.SLICE
            end
        end
    end
    if paths.poseSlice then
        return math.min(cost, 490000), ax, ay, ac, asine, paths.poseSlice * Tuning.SLICE
    end
    return math.min(cost, 490000), x, y, c, s, paths.steps * Tuning.SLICE
end

@native
local function risk(o: Obs, me: Tank, paths: Paths, drive: string, turn: string, straighten: number?)
    -- 每份观察仅有九种按键组合和一组拐后直走候选。整数索引避免内层拼接字符串。
    local key = (if drive == "forward" then 0 elseif drive == "reverse" then 3 else 6)
        + (if turn == "none" then 1 elseif turn == "left" then 2 else 3)
        + 9 * (straighten or 0)
    local memo = paths.memo[key]
    if not memo then
        memo = { rollout(o, me, paths, drive, turn, straighten) }
        if paths.guards then
            -- 与所有候选（含原按键、已承诺动作和拐后直走）用同一评分。近期实弹碰撞从 50 万起。
            local cap = if memo[1] >= 500000 then 12000 else 499000 - memo[1]
            memo[1] += math.min(cap, lostRoom(paths.guards, memo[2], memo[3], memo[6]) * 8)
        end
        paths.memo[key] = memo
    end
    return memo[1], memo[2], memo[3], memo[4], memo[5], memo[6]
end

-- 孤立交锋仍可收紧净距，但不再改变时域。
@native
local function chooseClearance(o: Obs, me: Tank)
    clearance = Tuning.BUFFER
    if o.ammo > 2 or #o.shells < 5 then
        return
    end
    local count, other: number? = 0, nil
    for _, b in o.shells do
        if (b.x - me.x) ^ 2 + (b.y - me.y) ^ 2 <= (2.3 * Frame.cell) ^ 2 then
            count += 1
            if b.owner ~= me.slot_id then
                if other and other ~= b.owner then
                    return
                end
                other = b.owner
            end
        end
    end
    if count < 5 or not other or not Tracks.bySlot[other] then
        return
    end
    for _, t in o.tanks do
        if t.slot_id == other then
            if t.team_id == me.team_id then
                return
            end
        elseif t.alive and t.slot_id ~= me.slot_id and (t.x - me.x) ^ 2 + (t.y - me.y) ^ 2 < (3 * Frame.cell) ^ 2 then
            return
        end
    end
    clearance = math.min(Tuning.BUFFER, 51)
end

return {
    chooseClearance = chooseClearance,
    forecast = forecast,
    lostRoom = lostRoom,
    prefire = prefire,
    prepareRisk = prepareRisk,
    score = risk,
}
