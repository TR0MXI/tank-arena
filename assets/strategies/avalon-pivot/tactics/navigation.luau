--!strict
local Frame = require("../model/frame")
local Geo = require("../model/geometry")
local Tuning = require("../model/tuning")
local Types = require("../model/types")

type Field = Types.Field
type Obs = Types.Obs
type Tank = Types.Tank

-- 观察可能跳步。停转后须超出较宽的死区才重新修舵；正在转则尽量对正。

@native
local function trim(o: Obs, err: number, navigation: boolean)
    local slack = Frame.spin * math.max(0.56, Frame.cadence * 0.5)
    if o.turn == "none" then
        slack += if navigation then 3 else 1
    end
    return Geo.turnFor(err, slack)
end

local function steer(o: Obs, me: Tank, x: number, y: number)
    local err = Geo.errorTo(me, x, y)
    local drive = "forward"
    if math.abs(err) > (if o.drive == "reverse" then 83 else 102) then
        err = (err + 256) % 256 - 128
        drive = "reverse"
    end
    -- 有转弯余量就边走边转；狭窄墙角先转正，不能带着旧角度切弯。
    local angle = me.heading * math.pi / 128
    local step = if drive == "reverse" then -185 else 220
    if
        math.abs(err) > 52
        or (math.abs(err) > 3 and Geo.cast(me.x, me.y, math.cos(angle) * step, math.sin(angle) * step, Frame.hull))
    then
        drive = "stop"
    end
    return drive, trim(o, err, true)
end

-- 由目标向外逐层标出格距；每次最多扩展 80 格，大片迷宫不独占一次决策。
-- 到达任意已标过的格子都能接着走，躲弹偏离路线后不必从头跑完整张图。
local field: Field?, waypoint: number?, bestGap: number, lastProgress: number
local avoided: number, avoidUntil = 0, 0
@native
local function laneClear(o: Obs, me: Tank, x: number, y: number, r: number)
    local dx, dy = x - me.x, y - me.y
    if Geo.cast(me.x, me.y, dx, dy, r) then
        return false
    end
    local len2 = math.max(1, dx * dx + dy * dy)
    for _, a in o.tanks do
        if a.alive and a.slot_id ~= me.slot_id and a.team_id == me.team_id then
            local projection = ((a.x - me.x) * dx + (a.y - me.y) * dy) / len2
            if projection <= 0 then
                continue
            end
            local t = math.min(projection, 1)
            if (me.x + t * dx - a.x) ^ 2 + (me.y + t * dy - a.y) ^ 2 < (2.6 * Frame.hull) ^ 2 then
                return false
            end
        end
    end
    return true
end

-- 每个（席位、路口、目的地、候选格）固定一个有界偏好；跳观察或重建路线不会重掷。
@native
local function preference(seed: number, here: number, goal: number, v: number): number
    local h = bit32.bxor(seed, here * 131071, goal * 65521, v * 8191)
    h = bit32.bxor(h, bit32.rshift(h, 16)) * 40503
    h = bit32.bxor(h, bit32.rshift(h, 13)) * 51893
    return bit32.band(bit32.bxor(h, bit32.rshift(h, 16)), 65535) / 65536
end

-- 候选没有第二个出口、被友车堵住、或将钻进近敌正面炮线时，不因温度改选它。
@native
local function openAlternative(o: Obs, me: Tank, v: number): boolean
    local x, y = Geo.center(v)
    if not laneClear(o, me, x, y, Frame.hull) then
        return false
    end
    local exits = 0
    for d = 1, 4 do
        local n = Geo.neighbor(v, d)
        if n then
            exits += 1
            if exits == 2 then
                break
            end
        end
    end
    if exits < 2 then
        return false
    end
    for _, t in o.tanks do
        if t.alive and t.slot_id ~= me.slot_id and t.team_id == me.team_id then
            local gap = (x - t.x) ^ 2 + (y - t.y) ^ 2
            if
                gap < (0.85 * Frame.cell) ^ 2
                and gap < (x - me.x) ^ 2 + (y - me.y) ^ 2
                and math.abs(Geo.errorTo(t, x, y)) < 24
            then
                return false
            end
        end
        if
            t.alive
            and t.team_id ~= me.team_id
            and (t.x - x) ^ 2 + (t.y - y) ^ 2 < (2.5 * Frame.cell) ^ 2
            and math.abs(Geo.errorTo(t, x, y)) < 26
            and Geo.direct(t.x, t.y, x, y, Frame.shell)
        then
            return false
        end
    end
    return true
end

@native
local function quietChoice(o: Obs, me: Tank): boolean
    for _, s in o.shells do
        if (s.x - me.x) ^ 2 + (s.y - me.y) ^ 2 < (2.5 * Frame.cell) ^ 2 then
            return false
        end
    end
    return true
end

-- 一整段通道的入弯线，只在切换路点时选择。到达提前量和卡住判定也使用同一目标点。
local laneX: number?, laneY: number? = nil, nil
@native
local function chooseLane(o: Obs, me: Tank, here: number, v: number, goal: number): (number?, number?)
    local seed = o.behavior_seed
    if not seed or Tuning.ROUTE_TEMP <= 0 or v == here then
        return nil, nil
    end
    local x, y = Geo.center(v)
    local bx, by = Geo.center(here)
    local dx, dy = x - bx, y - by
    local len = math.sqrt(dx * dx + dy * dy)
    local draw = preference(seed, here, goal + 4096, v)
    if draw > 0.33 and draw < 0.67 or not quietChoice(o, me) then
        return nil, nil
    end
    local side = (if draw <= 0.33 then -1 else 1) * math.min(0.22, 0.18 * Tuning.ROUTE_TEMP) * Frame.cell / len
    local px, py = x - dy * side, y + dx * side
    -- 用整车包络验证进入和沿原方向离开的空间；不以擦墙、撞队友换取花样。
    if not openAlternative(o, me, v) then
        return nil, nil
    end
    -- 转弯前必须有整车旋转空间；余地不足继续原来沿格心的路线，不试侧偏。
    local body = math.max(
        math.sqrt(o.hull_half_length ^ 2 + Frame.hull ^ 2),
        o.turret_offset + o.turret_half_length
    ) + 12
    if math.abs(Geo.errorTo(me, px, py)) > 30 and Geo.cast(me.x, me.y, 0, 0, body) then
        return nil, nil
    end
    if
        laneClear(o, me, px, py, body)
        and Geo.direct(px, py, px + dx / len * 0.22 * Frame.cell, py + dy / len * 0.22 * Frame.cell, body)
    then
        return px, py
    end
    return nil, nil
end

local function startField(goal: number, here: number, me: Tank, o: Obs, now: number, ignoreTraffic: boolean)
    local blocked = {}
    if not ignoreTraffic then
        for _, t in o.tanks do
            if t.alive and t.team_id == me.team_id and t.slot_id ~= me.slot_id then
                blocked[Geo.cell(t.x, t.y)] = true
            end
        end
        if now < avoidUntil then
            blocked[avoided] = true
        end
    end
    blocked[here], blocked[goal] = nil, nil
    field = { goal = goal, dist = { [goal] = 0 }, q = { goal }, head = 1, blocked = blocked }
    waypoint, laneX, laneY = nil, nil, nil
end

@native
local function navigate(o: Obs, me: Tank, goal: number, now: number)
    local here = Geo.cell(me.x, me.y)
    -- 移动目标换格时不要立刻扔掉一份刚算完的大图路线：边接近，边定期重选。
    if not field or (field.goal ~= goal and field.retargetAt and now >= field.retargetAt) then
        startField(goal, here, me, o, now, false)
    end
    local f = field :: Field
    for _ = 1, 80 do
        if f.dist[here] or f.head > #f.q then
            break
        end
        local id = f.q[f.head]
        f.head += 1
        for d = 1, 4 do
            local v = Geo.neighbor(id, d)
            if v and not f.blocked[v] and not f.dist[v] then
                f.dist[v] = f.dist[id] + 1
                table.insert(f.q, v)
            end
        end
    end
    if not f.dist[here] then
        if f.head > #f.q then
            startField(goal, here, me, o, now, true)
        end
        return "stop", "none"
    end
    f.retargetAt = f.retargetAt or now + 35
    if waypoint then
        local x, y = Geo.center(waypoint)
        if laneX and laneY then
            x, y = laneX, laneY
        end
        local gap = (me.x - x) ^ 2 + (me.y - y) ^ 2
        if gap < 100 ^ 2 or not f.dist[waypoint] or f.dist[waypoint] > f.dist[here] then
            waypoint = nil
        elseif gap < bestGap - 32 ^ 2 then
            bestGap, lastProgress = gap, now
        elseif now - lastProgress > 50 then
            avoided, avoidUntil = waypoint, now + 90
            startField(goal, here, me, o, now, false)
        end
    end
    if not waypoint then
        local active = field :: Field
        local best = active.dist[here]
        local effort = math.huge
        if best then
            for i = 1, 4 do
                local v = Geo.neighbor(here, i)
                if v and active.dist[v] and active.dist[v] < active.dist[here] then
                    local x, y = Geo.center(v)
                    local angle = math.abs(Geo.errorTo(me, x, y))
                    if active.dist[v] < best or (active.dist[v] == best and angle < effort) then
                        waypoint, best, effort = v, active.dist[v], angle
                    end
                end
            end
        end
        local rerouted = false
        local seed = o.behavior_seed
        if seed and Tuning.ROUTE_TEMP > 0 and waypoint and best and waypoint ~= here then
            local width = 104 * Tuning.ROUTE_TEMP
            local original = waypoint :: number
            local rank = -effort + width * preference(seed, here, goal, original)
            for d = 1, 4 do
                local v = Geo.neighbor(here, d)
                if v and v ~= original and active.dist[v] == best then
                    local x, y = Geo.center(v)
                    local quality = -math.abs(Geo.errorTo(me, x, y))
                    local candidate = quality + width * preference(seed, here, goal, v)
                    if candidate > rank and quietChoice(o, me) and openAlternative(o, me, v) then
                        waypoint, rank, rerouted = v, candidate, true
                    end
                end
            end
        end
        -- 看穿下一个转角才抄近路；不能用点状车心切过墙端或队友的格子。
        if not rerouted and waypoint and waypoint ~= here then
            for d = 1, 4 do
                local v = Geo.neighbor(waypoint, d)
                if v and active.dist[v] and active.dist[v] < best then
                    local x, y = Geo.center(v)
                    if laneClear(o, me, x, y, o.turret_offset + o.turret_half_length) then
                        waypoint = v
                        break
                    end
                end
            end
        end
        local nextWaypoint = waypoint or here
        waypoint = nextWaypoint
        laneX, laneY = chooseLane(o, me, here, nextWaypoint, goal)
        bestGap, lastProgress = math.huge, now
    end
    local x, y = Geo.center(waypoint or here)
    if laneX and laneY then
        x, y = laneX, laneY
    end
    -- 从拐角先回到本格中心，再驶向相邻格；车体不能像路径节点一样贴墙拐弯。
    if waypoint ~= here and not Geo.direct(me.x, me.y, x, y, 180) then
        x, y = Geo.center(here)
    end
    if (me.x - x) ^ 2 + (me.y - y) ^ 2 < 64 ^ 2 then
        return "stop", "none"
    end
    return steer(o, me, x, y)
end

local function reset()
    field, waypoint = nil, nil
end

return {
    laneClear = laneClear,
    navigate = navigate,
    openAlternative = openAlternative,
    preference = preference,
    quietChoice = quietChoice,
    trim = trim,
    reset = reset,
}
