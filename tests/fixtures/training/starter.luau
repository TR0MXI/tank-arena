-- starter · 历史陪练样本，用于回归、录制和联赛，不随游戏发布。
-- 两个示例共用的驾驶工具，只读取公开的 obs。每席闭包缓存自己的地图和航点。
local CELL = 1024
local directions = { { 1, 0 }, { 0, 1 }, { -1, 0 }, { 0, -1 } }
local graph, cols, rows
local waypoint, progressAt, progressGap, avoidCell, avoidUntil
local lastTargetX, lastTargetY, lastTargetId, lastSeen
local blindSince, moveUntil = nil, 0
local function cell(x, y)
    return math.floor(y / CELL) * cols + math.floor(x / CELL) + 1
end

local function center(id)
    return ((id - 1) % cols + 0.5) * CELL, (math.floor((id - 1) / cols) + 0.5) * CELL
end

local function loadMap(maze)
    local lines = string.split(maze, "\n")
    cols = math.floor(#lines[1] / 2)
    rows = math.floor(#lines / 2)
    graph = table.create(cols * rows)
    for y = 0, rows - 1 do
        for x = 0, cols - 1 do
            local exits = {}
            graph[y * cols + x + 1] = exits
            for _, d in directions do
                local u, v = x + d[1], y + d[2]
                if u < 0 or v < 0 or u >= cols or v >= rows then
                    continue
                end
                local pass = string.byte(lines[2 * y + 2 + d[2]], 2 * x + 2 + d[1]) == 32
                if pass then
                    table.insert(exits, v * cols + u + 1)
                end
            end
        end
    end
end

local function openEdge(from, to)
    return table.find(graph[from], to) ~= nil
end

-- 沿格线追踪视线；恰好擦过墙角时保守地放弃射击路线。
local function clear(x, y, tx, ty)
    local id, goal = cell(x, y), cell(tx, ty)
    local dx, dy = tx - x, ty - y
    local sx, sy = math.sign(dx), math.sign(dy)
    local ax, ay =
        if dx == 0 then math.huge else CELL / math.abs(dx), if dy == 0 then math.huge else CELL / math.abs(dy)
    local ex = if dx == 0 then math.huge else (if sx > 0 then CELL - x % CELL else x % CELL) / math.abs(dx)
    local ey = if dy == 0 then math.huge else (if sy > 0 then CELL - y % CELL else y % CELL) / math.abs(dy)
    for _ = 1, cols + rows do
        if id == goal then
            return true
        end
        if math.abs(ex - ey) < 1e-6 then
            if
                not (
                    openEdge(id, id + sx)
                    and openEdge(id, id + sy * cols)
                    and openEdge(id + sx, id + sx + sy * cols)
                    and openEdge(id + sy * cols, id + sx + sy * cols)
                )
            then
                return false
            end
            id += sx + sy * cols
            ex += ax
            ey += ay
        elseif ex < ey then
            if not openEdge(id, id + sx) then
                return false
            end
            id += sx
            ex += ax
        else
            if not openEdge(id, id + sy * cols) then
                return false
            end
            id += sy * cols
            ey += ay
        end
    end
    return id == goal
end

-- 格线没有厚度，但真实墙有。沿射线两侧检查余量，别停在墙端后面架炮。
local function firingLane(x, y, tx, ty)
    local vx, vy = tx - x, ty - y
    local len = math.sqrt(vx * vx + vy * vy)
    if len < 1 then
        return true
    end
    local dx, dy = -vy * 96 / len, vx * 96 / len
    return clear(x, y, tx, ty)
        and clear(x + dx, y + dy, tx + dx, ty + dy)
        and clear(x - dx, y - dy, tx - dx, ty - dy)
end

local function angleError(me: { x: number, y: number, heading: number }, x: number, y: number): number
    return (math.atan2(y - me.y, x - me.x) * 128 / math.pi - me.heading + 128) % 256 - 128
end

local function steer(o, me, x, y, reverse)
    local e = angleError(me, x, y)
    if reverse then
        e = (e + 256) % 256 - 128
    end
    local slack = math.max(1, o.turn_step / 2)
    return {
        turn = if e > slack then "right" elseif e < -slack then "left" else "none",
        drive = if math.abs(e) > 20 then "stop" elseif reverse then "reverse" else "forward",
    }
end

local function distance2(me, x, y)
    return (me.x - x) ^ 2 + (me.y - y) ^ 2
end

local function threat(o, me)
    local first, nearest = nil, math.huge
    for _, s in o.shells do
        local v2 = s.vx ^ 2 + s.vy ^ 2
        if v2 == 0 then
            continue
        end
        local dx, dy = me.x - s.x, me.y - s.y
        local t = (dx * s.vx + dy * s.vy) / v2
        if
            t > 0
            and t < nearest
            and t < math.min(16, s.life)
            and (dx - t * s.vx) ^ 2 + (dy - t * s.vy) ^ 2 < (o.hit_radius + 64) ^ 2
            and clear(s.x, s.y, me.x, me.y)
        then
            first, nearest = s, t
        end
    end
    return first
end

local function evade(o, me, s)
    local best, score = nil, -math.huge
    for _, id in graph[cell(me.x, me.y)] do
        local x, y = center(id)
        local side = math.abs((x - me.x) * s.vy - (y - me.y) * s.vx) / (o.shell_speed * CELL)
        local e = math.abs(angleError(me, x, y))
        local reverse = e > 64
        local turn = math.min(e, 128 - e)
        local value = side - turn / 90
        if value > score then
            best, score = steer(o, me, x, y, reverse), value
        end
    end
    return best
end

-- 从当前格心一次广搜，得到实际可达的首跳，而不是朝隔墙敌人硬开。
local first, dist, queue, occupied = {}, {}, {}, {}
local function routes(o, me, source)
    -- 每个席位独占一份搜索空间；清空内容，保留上次广搜分配的容量。
    table.clear(first)
    table.clear(dist)
    table.clear(queue)
    table.clear(occupied)
    dist[source], queue[1] = 0, source

    for _, t in o.tanks do
        if t.alive and t.slot_id ~= me.slot_id and t.team_id == me.team_id then
            occupied[cell(t.x, t.y)] = true
        end
    end
    local head = 1
    while head <= #queue do
        local at = queue[head]
        head += 1
        for _, to in graph[at] do
            if dist[to] or (at == source and ((to == avoidCell and o.tick < avoidUntil) or occupied[to])) then
                continue
            end
            first[to] = first[at] or to
            dist[to] = dist[at] + 1
            table.insert(queue, to)
        end
    end
    return first, dist, queue
end

local function nearestReachableEnemy(enemies, dist)
    local goal, cost = nil, math.huge
    for _, enemy in enemies do
        local id = cell(enemy.x, enemy.y)
        if dist[id] and dist[id] < cost then
            goal, cost = id, dist[id]
        end
    end
    return goal
end

local function navigate(o, me, opponents, choose)
    local from = cell(me.x, me.y)
    if waypoint then
        local x, y = center(waypoint)
        local gap = math.sqrt(distance2(me, x, y))
        if gap < 84 then
            waypoint = nil
        elseif gap < progressGap - 36 then
            progressGap, progressAt = gap, o.tick
        elseif o.tick - progressAt > o.tick_hz * 1.4 then
            avoidCell, avoidUntil = waypoint, o.tick + o.tick_hz * 2
            waypoint = nil
        end
    end
    if not waypoint then
        local first, dist, reachable = routes(o, me, from)
        local goal = choose(opponents, dist, reachable)
        waypoint = (goal and first[goal]) or reachable[2]
        if not waypoint then
            -- 全部相邻格暂时被占：退回本格中心，让队友通过，再重新规划。
            local x, y = center(from)
            return steer(o, me, x, y, math.abs(angleError(me, x, y)) > 64)
        end
        progressGap, progressAt = math.huge, o.tick
    end
    local x, y = center(waypoint)
    return steer(o, me, x, y, false)
end

local function closestVisible(me, enemies)
    local visible, range = nil, math.huge
    for _, t in enemies do
        local d = distance2(me, t.x, t.y)
        if d < range and firingLane(me.x, me.y, t.x, t.y) then
            visible, range = t, d
        end
    end
    return visible, math.sqrt(range)
end

local function predictAim(o, me, visible, range)
    local x, y = visible.x, visible.y
    if visible.slot_id == lastTargetId and lastSeen and o.tick > lastSeen then
        local flight = math.min(range / o.shell_speed, 15)
        local elapsed = o.tick - lastSeen
        local px = x + math.clamp((x - lastTargetX) / elapsed, -o.forward_speed, o.forward_speed) * flight
        local py = y + math.clamp((y - lastTargetY) / elapsed, -o.forward_speed, o.forward_speed) * flight
        if px >= 0 and py >= 0 and px < cols * CELL and py < rows * CELL and clear(me.x, me.y, px, py) then
            x, y = px, py
        end
    end
    lastTargetX, lastTargetY, lastTargetId, lastSeen = visible.x, visible.y, visible.slot_id, o.tick
    return x, y
end

local function updateFiringPosition(o)
    -- 中心线近似可见而实际炮线被墙端或队友挡住时，不能一直架炮。
    if o.enemy_ahead then
        blindSince = nil
    elseif not blindSince then
        blindSince = o.tick
    end
    if blindSince and o.tick - blindSince > o.tick_hz * 1.4 then
        moveUntil = o.tick + o.tick_hz
        blindSince = nil
        waypoint = nil
    end
end

local function pilot(choose, engage)
    local enemies = {}
    return function(o)
        if not graph then
            loadMap(o.maze)
        end

        local me = o.me
        table.clear(enemies)
        for _, t in o.tanks do
            if t.alive and t.team_id ~= o.team_id then
                table.insert(enemies, t)
            end
        end
        if #enemies == 0 then
            return { drive = "stop", turn = "none" }
        end

        local command, aimX, aimY
        local visible, range = closestVisible(me, enemies)
        if visible then
            aimX, aimY = predictAim(o, me, visible, range)
            updateFiringPosition(o)
            if o.tick >= moveUntil then
                command = engage(o, me, aimX, aimY, range)
            end
        else
            blindSince, lastSeen = nil, nil
        end

        -- 最终用裁判的实际炮线（含队友、墙厚与炮口）复核；看见敌人不等于可以开炮。
        if o.can_fire and o.enemy_ahead and aimX and math.abs(angleError(me, aimX, aimY)) < 4 then
            -- 按当前炮线扣扳机；下一份决策显式松开，才可继续点射。
            return { turn = "none", drive = "stop", fire = true }
        end

        local incoming = threat(o, me)
        if incoming then
            command = evade(o, me, incoming) or command
        end
        command = command or navigate(o, me, enemies, choose)
        if o.fire_down then
            command.fire = false
        end
        return command
    end
end

-- 追击手：选路程最近的敌车，不执着于直线最近但隔着墙的对手。
local function closeIn(o, me, x, y, range)
    -- 远处朝向接近时继续按格心追，不沿墙角抄对角线。
    if range > 2.2 * CELL and not o.enemy_ahead then
        return nil
    end
    local act = steer(o, me, x, y, false)
    if range < 0.65 * CELL then
        act.drive = "stop"
    end
    return act
end

return pilot(nearestReachableEnemy, closeIn)
