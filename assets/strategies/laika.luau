-- @name 莱卡（Laika）
-- @author Tank Arena
-- @description 莱卡风格的迷宫陪练：找反弹角、躲来弹、空仓撤退。只用公开观察，不读取裁判状态。

local abs, min, max, floor = math.abs, math.min, math.max, math.floor
local function length(x, y)
    return math.sqrt(x * x + y * y)
end
-- 官网 2026-11-19 版 Laika 配置中，本模式有对应机制的五个特质。
local profile =
    { cleverness = 0.4, dexterity = 0.6, boldness = 0.9, aggressiveness = 0.9, determination = 0.6 }
local fireRange = 2 + 6 * profile.cleverness -- 候选弹道长度（格），不是可见距离
local fireBounces = math.ceil(1 + 5 * profile.cleverness)
local dodgeBounces = math.ceil(1 + 4 * profile.cleverness)
local rad = 2 * math.pi / 256 -- obs.heading 顺时针，一圈 256
local function bearing(x, y)
    return math.atan2(y, x) / rad
end
local function errorTo(a, b)
    return (b - a + 128) % 256 - 128
end
-- 地图和满弹幕分析可跨观察分片；通常的小局面会在当前观察内完成。
local fuel
local function charge(n)
    if fuel then
        fuel -= n
        if fuel <= 0 then
            coroutine.yield()
        end
    end
end
local rows, width, height, cell
local horizontal, vertical = {}, {}
local function tile(x, y)
    return floor(x / cell) + width * floor(y / cell)
end
local function center(id)
    return (id % width + 0.5) * cell, (floor(id / width) + 0.5) * cell
end

-- ASCII 地图全图可查；墙按格线懒解析并合并共线段，不为每颗子弹遍历整张地图。
local function wallLine(lines, index, isVertical, h)
    if lines[index] then
        return lines[index]
    end
    local walls, start = {}, nil
    local count = if isVertical then height else width
    charge(count * 2)
    for j = 0, count do
        local solid = j < count
            and (
                if isVertical
                    then string.byte(rows[2 * j + 2], 2 * index + 1) == 124
                    else string.byte(rows[2 * index + 1], 2 * j + 2) == 45
            )
        if solid then
            start = start or j
        elseif start then
            local lo, hi, across = start * cell - h, j * cell + h, index * cell
            table.insert(
                walls,
                if isVertical then { across - h, lo, across + h, hi } else { lo, across - h, hi, across + h }
            )
            start = nil
        end
    end
    lines[index] = walls
    return walls
end
local wallHalf
-- 射线与膨胀墙矩形的首次相交。平面按法向反射；圆角/同时碰两面不猜反弹角。
local function cast(x, y, ux, uy, limit, radius)
    local best, axis = limit, nil
    local ex, ey = x + ux * limit, y + uy * limit
    local xl, xh, yl, yh = min(x, ex) - radius, max(x, ex) + radius, min(y, ey) - radius, max(y, ey) + radius
    local function inspect(walls)
        charge(2 + #walls * 2)
        for _, w in walls do
            if w[1] > xh or w[3] < xl or w[2] > yh or w[4] < yl then
                continue
            end
            local ax, bx, ay, by = -math.huge, math.huge, -math.huge, math.huge
            if abs(ux) > 1e-8 then
                ax, bx = (w[1] - radius - x) / ux, (w[3] + radius - x) / ux
                if ax > bx then
                    ax, bx = bx, ax
                end
            end
            if abs(uy) > 1e-8 then
                ay, by = (w[2] - radius - y) / uy, (w[4] + radius - y) / uy
                if ay > by then
                    ay, by = by, ay
                end
            end
            local at = max(0, ax, ay)
            if at > best or at > min(bx, by) then
                continue
            end
            local side = 0
            if ax > ay and y + uy * at >= w[2] and y + uy * at <= w[4] then
                side = 1
            end
            if ay > ax and x + ux * at >= w[1] and x + ux * at <= w[3] then
                side = 2
            end
            if axis and abs(at - best) < 0.001 and side ~= axis then
                side = 0
            end
            best, axis = at, side
        end
    end
    for idx = max(0, math.ceil((yl - wallHalf) / cell)), min(height, floor((yh + wallHalf) / cell)) do
        inspect(wallLine(horizontal, idx, false, wallHalf))
    end
    for idx = max(0, math.ceil((xl - wallHalf) / cell)), min(width, floor((xh + wallHalf) / cell)) do
        inspect(wallLine(vertical, idx, true, wallHalf))
    end
    return best, axis
end

-- 公用的预测射线。官网规划器用中心线，不模拟每一物理 tick 的弹丸圆形碰撞体。
local function trace(x, y, ux, uy, limit, bounces, visit)
    local traveled = 0
    for leg = 0, bounces do
        local d, axis = cast(x, y, ux, uy, limit - traveled, 0)
        if visit(x, y, ux, uy, d, traveled, leg, axis) or not axis or axis == 0 or d < 0.001 then
            return
        end
        x, y = x + ux * d, y + uy * d
        if axis == 1 then
            ux = -ux
        else
            uy = -uy
        end
        x, y = x + ux * 0.02, y + uy * 0.02
        traveled += d + 0.02
        if traveled >= limit then
            return
        end
    end
end
local function visible(x, y, tx, ty)
    local dx, dy = tx - x, ty - y
    local d = length(dx, dy)
    if d < 0.001 then
        return true
    end
    local _, wall = cast(x, y, dx / d, dy / d, d, 0)
    return not wall
end

-- 预测首个坦克接触：车身及前伸炮塔是两个随车旋转的矩形。
local function boxTime(x, y, u, v, hx, hy)
    local lo, hi = 0, math.huge
    if abs(u) < 1e-9 then
        if abs(x) > hx then
            return math.huge
        end
    else
        local l, r = (-hx - x) / u, (hx - x) / u
        lo, hi = max(lo, min(l, r)), min(hi, max(l, r))
    end
    if abs(v) < 1e-9 then
        if abs(y) > hy then
            return math.huge
        end
    else
        local l, r = (-hy - y) / v, (hy - y) / v
        lo, hi = max(lo, min(l, r)), min(hi, max(l, r))
    end
    return if lo <= hi then lo else math.huge
end
local function firingLine(o, angle, checkNear)
    local ux, uy = math.cos(angle * rad), math.sin(angle * rad)
    local nearest, travel, unsafe = math.huge, math.huge, false
    trace(
        o.me.x + ux * o.hull_half_length,
        o.me.y + uy * o.hull_half_length,
        ux,
        uy,
        fireRange * cell,
        fireBounces,
        function(x, y, u, v, d, start, leg, axis)
            charge(6 * #o.tanks)
            local hit, at = nil, d
            for _, t in o.tanks do
                if not t.alive or (leg == 0 and t.slot_id == o.slot_id) then
                    continue
                end
                local dx, dy = x - t.x, y - t.y
                local cs, sn = math.cos(t.heading * rad), math.sin(t.heading * rad)
                local lx, ly = dx * cs + dy * sn, dy * cs - dx * sn
                local a, b = u * cs + v * sn, v * cs - u * sn
                local h = min(
                    boxTime(lx, ly, a, b, o.hull_half_length, o.hull_half_width),
                    boxTime(lx - o.turret_offset, ly, a, b, o.turret_half_length, o.turret_half_width)
                )
                if h < at then
                    hit, at = t, h
                end
                if checkNear and t.team_id ~= o.team_id then
                    local proj = math.clamp(-dx * u - dy * v, 0, d)
                    local px, py = x + u * proj, y + v * proj
                    local gap = length(px - t.x, py - t.y)
                    if gap < nearest and visible(px, py, t.x, t.y) then
                        nearest, travel = gap, start + proj
                    end
                end
            end
            if hit then
                unsafe = hit.team_id == o.team_id
                nearest, travel = 0, start + at
                return true
            elseif leg == 0 and axis and d < 0.4 * cell then
                unsafe = true -- 先贴脸撞墙的非激光路线按自伤丢弃。
                return true
            end
            return false
        end
    )
    return if unsafe then math.huge else nearest, travel, unsafe
end

-- 四邻通道距离用于选目标；驱动路径可沿畅通方格对角线走，不能切过墙角。
local adjacency, diagonal, fields, fieldKeys = {}, {}, {}, {}
local function edges(id)
    if adjacency[id] then
        return adjacency[id]
    end
    charge(5)
    local a = {}
    local x, y = 2 * (id % width) + 2, 2 * floor(id / width) + 2
    if string.byte(rows[y], x - 1) == 32 then
        table.insert(a, id - 1)
    end
    if string.byte(rows[y], x + 1) == 32 then
        table.insert(a, id + 1)
    end
    if string.byte(rows[y + 1], x) == 32 then
        table.insert(a, id + width)
    end
    if string.byte(rows[y - 1], x) == 32 then
        table.insert(a, id - width)
    end
    adjacency[id] = a
    return a
end
local function steps(id)
    if diagonal[id] then
        return diagonal[id]
    end
    local card = edges(id)
    local a = table.clone(card)
    for _, h in card do
        if abs(h - id) ~= 1 then
            continue
        end
        for _, v in card do
            if abs(v - id) ~= width then
                continue
            end
            local corner = h + v - id
            if table.find(edges(h), corner) and table.find(edges(v), corner) then
                table.insert(a, corner)
            end
        end
    end
    diagonal[id] = a
    return a
end
local function distances(root, depth)
    local cached = fields[root]
    if cached and cached.depth >= depth then
        return cached.d
    end
    local d, q, head = { [root] = 0 }, { root }, 1
    while head <= #q do
        charge(6)
        local id = q[head]
        head += 1
        if d[id] >= depth then
            break
        end
        for _, n in edges(id) do
            if not d[n] then
                d[n] = d[id] + 1
                table.insert(q, n)
            end
        end
    end
    if not cached then
        if #fieldKeys >= 12 then
            fields[table.remove(fieldKeys, 1)] = nil
        end
        table.insert(fieldKeys, root)
    end
    fields[root] = { d = d, depth = depth }
    return d
end
-- 把轨迹分割到实际穿过的格子。每段以其中点估计到达时间；权重以官网的米计（每格 10 米）。
local function paint(map, x, y, u, v, d, start, weight)
    local dx, dy = math.sign(u), math.sign(v)
    local ix, iy = floor(x / cell), floor(y / cell)
    local tx = if abs(u) < 1e-8 then math.huge else ((ix + (if dx > 0 then 1 else 0)) * cell - x) / u
    local ty = if abs(v) < 1e-8 then math.huge else ((iy + (if dy > 0 then 1 else 0)) * cell - y) / v
    local sx, sy = cell / max(abs(u), 1e-8), cell / max(abs(v), 1e-8)
    local t = 0
    while t < d do
        charge(3)
        local last = min(d, tx, ty)
        if ix >= 0 and ix < width and iy >= 0 and iy < height and last > t then
            local id = ix + iy * width
            map[id] = (map[id] or 0) + (last - t) * 10 / cell * weight(id, start + (t + last) / 2)
        end
        if last >= d then
            break
        end
        if tx <= last then
            ix += dx
            tx += sx
        end
        if ty <= last then
            iy += dy
            ty += sy
        end
        t = last
    end
end
local function assess(o, dist)
    local heat, worst = {}, nil
    local missileRange = 4 + 6 * profile.cleverness
    local scare = cell * (15 - 12 * profile.boldness) / 10
    for _, s in o.shells do
        local origin = tile(s.x, s.y)
        if not dist[origin] or dist[origin] > missileRange then
            continue
        end
        local speed = length(s.vx, s.vy)
        if speed == 0 then
            continue
        end
        local nearest = nil
        trace(
            s.x,
            s.y,
            s.vx / speed,
            s.vy / speed,
            min(s.life * speed, missileRange * cell),
            dodgeBounces,
            function(x, y, u, v, d, start, leg)
                paint(heat, x, y, u, v, d, start, function(id, path)
                    local separation = abs(path / speed - (dist[id] or math.huge) * cell / o.forward_speed)
                        / o.tick_hz
                    return 0.5 * max(0, 1 - 0.25 * separation)
                end)
                if leg == 0 and s.owner == o.slot_id and not s.deadly_to_owner then
                    return
                end
                if nearest and nearest.gap < scare then
                    return
                end
                local along = (o.me.x - x) * u + (o.me.y - y) * v
                if along < 0 then
                    return
                end
                along = min(along, d)
                local px, py = x + u * along, y + v * along
                local gap = length(o.me.x - px, o.me.y - py)
                if (not nearest or gap < nearest.gap) and visible(px, py, o.me.x, o.me.y) then
                    nearest = {
                        gap = gap,
                        time = (start + along) / speed,
                        x = px,
                        y = py,
                        u = u,
                        v = v,
                        origin = origin,
                    }
                end
            end
        )
        if nearest and (not worst or nearest.gap < worst.gap) then
            worst = nearest
        end
    end
    for _, t in o.tanks do
        if not t.alive or t.team_id == o.team_id then
            continue
        end
        local id = tile(t.x, t.y)
        if not dist[id] or dist[id] > 8 + 8 * profile.cleverness then
            continue
        end
        heat[id] = (heat[id] or 0) + 2
        for _, n in edges(id) do
            heat[n] = (heat[n] or 0) + 1
        end
        local u, v = math.cos(t.heading * rad), math.sin(t.heading * rad)
        local limit = (2 + 8 * profile.cleverness) * cell
        trace(
            t.x + u * o.hull_half_length,
            t.y + v * o.hull_half_length,
            u,
            v,
            limit,
            math.ceil(2 + 3 * profile.cleverness),
            function(x, y, ux, uy, d, start)
                paint(heat, x, y, ux, uy, d, start, function(_, p)
                    return 0.25 * (1 - p / limit)
                end)
            end
        )
    end
    return heat, worst
end

-- 小顶堆 A*。危险格的进入代价远高于绕行一步；在大图分片搜索，不穿墙抄近路。
local function route(root, goal, heat)
    if root == goal then
        return {}
    end
    local cost, parent, heap = { [root] = 0 }, {}, {}
    local function heuristic(id)
        local dx, dy = abs(id % width - goal % width), abs(floor(id / width) - floor(goal / width))
        return 0.1 * (max(dx, dy) + 0.414214 * min(dx, dy))
    end
    local function push(id, g)
        local item = { id, g, g + heuristic(id) }
        local i = #heap + 1
        while i > 1 do
            local j = floor(i / 2)
            if heap[j][3] <= item[3] then
                break
            end
            heap[i] = heap[j]
            i = j
        end
        heap[i] = item
    end
    push(root, 0)
    while #heap > 0 do
        charge(20)
        local top, tail = heap[1], table.remove(heap)
        if #heap > 0 then
            local i = 1
            while 2 * i <= #heap do
                local j = 2 * i
                if j < #heap and heap[j + 1][3] < heap[j][3] then
                    j += 1
                end
                if heap[j][3] >= tail[3] then
                    break
                end
                heap[i] = heap[j]
                i = j
            end
            heap[i] = tail
        end
        local id = top[1]
        if top[2] ~= cost[id] then
            continue
        end
        if id == goal then
            break
        end
        for i, n in steps(id) do
            local c = top[2] + (if i <= #edges(id) then 0.1 else 0.1414214) + (heat[n] or 0)
            if c < (cost[n] or math.huge) then
                cost[n], parent[n] = c, id
                push(n, c)
            end
        end
    end
    local path, id = {}, goal
    while parent[id] do
        charge(1)
        table.insert(path, id)
        id = parent[id]
    end
    for i = 1, floor(#path / 2) do
        local j = #path - i + 1
        path[i], path[j] = path[j], path[i]
    end
    return path
end
local penalties = {}
local function deadEnd(id)
    if penalties[id] then
        return penalties[id]
    end
    local score = 0
    if #edges(id) == 1 then
        score = 5
    elseif #edges(id) == 2 then
        for _, first in edges(id) do
            local prev, n = id, first
            for d = 1, 4 do
                local a = edges(n)
                if #a == 1 then
                    score = max(score, 5 - d)
                    break
                end
                if #a ~= 2 then
                    break
                end
                prev, n = n, if a[1] == prev then a[2] else a[1]
            end
        end
    end
    penalties[id] = score
    return score
end
local function escape(root, sources, heat, dist)
    local fieldsNear = {}
    for _, source in sources do
        -- 四次对角移动至多增加八个四邻步；这些候选点的距离都可精确求出。
        fieldsNear[source] = distances(source, dist[source] + 8)
    end
    local function value(id)
        local sum = 0
        for _, source in sources do
            sum += fieldsNear[source][id]
        end
        return sum / max(1, #sources)
            - deadEnd(id) * (0.2 + 0.8 * profile.cleverness)
            - (heat[id] or 0) * (1 - 0.9 * profile.boldness)
    end
    local path, here = {}, root
    for _ = 1, math.ceil(1 + 6 * profile.cleverness) do
        local nextId, score = here, value(here)
        for _, n in steps(here) do
            local v = value(n)
            if v > score then
                nextId, score = n, v
            end
        end
        if nextId == here then
            break
        end
        table.insert(path, nextId)
        here = nextId
    end
    return path
end

-- 每个席位自己的可复现随机流；不同 seed 影响扇形抖动、转向误差和闲逛，不改变视野。
local seed
local function random()
    seed = (seed * 48271) % 2147483647
    return seed / 2147483647
end
local function noise()
    return (random() - 0.5) * 0.3 * (1 - profile.dexterity) / rad
end
local actions = {} -- 仅规划器写；完整队列才交给执行器。
local active, head = {}, 1
local function enqueue(a)
    table.insert(actions, a)
end
local function rotate(angle)
    enqueue({ kind = "turn", angle = angle + noise() })
end
local function moveTo(x, y, reverse, tolerance)
    enqueue({ kind = "move", x = x, y = y, reverse = reverse, close = tolerance, offset = noise() })
end
local function follow(path, target)
    for _, id in path do
        charge(4)
        local x, y = center(id)
        moveTo(x, y, #path <= 1, 0.4 * cell)
    end
    if target and path[#path] == target then
        local x, y = center(target)
        moveTo(x, y, #path <= 1, 0.1 * cell)
    end
end
local aggression = 0
local function attack(o, t)
    aggression = max(0, aggression - 0.3)
    local me = o.me
    local angle, fire = nil, false
    if visible(me.x, me.y, t.x, t.y) then
        angle, fire = bearing(t.x - me.x, t.y - me.y), true
    else
        local gap, shortest = math.huge, math.huge
        local count = math.ceil(1 + 4 * profile.cleverness)
        if count % 2 == 0 then
            count += 1
        end
        local spread = 1.04 + (2.09 - 1.04) * profile.aggressiveness
        for i = 1, count do
            local a = me.heading
                + ((i - 1) / (count - 1) - 0.5) * spread / rad
                + (random() - 0.5) * 0.35 / rad
            local near, path = firingLine(o, a, true)
            if near < gap or (near == 0 and gap == 0 and path < shortest) then
                angle, gap, shortest = a, near, path
            end
        end
        fire = gap < (8 + 12 * profile.aggressiveness) * cell / 10
        if not angle then
            angle = me.heading + (if random() < 0.5 then -1 else 1) * (1.04 + 1.05 * random()) / rad
        end
    end
    rotate(angle)
    if fire then
        enqueue({ kind = "fire", delay = 0.3 * (1 - profile.dexterity) })
    end
end
local function retaliate(o)
    if o.ammo == 0 then
        return
    end
    local gap = firingLine(o, o.me.heading, true)
    if gap < (4 + 6 * profile.aggressiveness) * cell / 10 then
        table.insert(actions, 1, { kind = "fire", delay = 0.1 * (1 - profile.dexterity) })
        aggression = max(0, aggression - 0.2)
    end
end
local function dodge(o, info, root, heat, dist)
    local urgent = info.time < o.tick_hz and info.gap < 0.8 * cell
    local path = if urgent then {} else escape(root, { info.origin }, heat, dist)
    if #path == 0 then
        if info.gap < 0.4 * cell then
            local dx, dy = -info.v * 0.4 * cell, info.u * 0.4 * cell
            local function cost(x, y)
                return (x - o.me.x) ^ 2
                    + (y - o.me.y) ^ 2
                    + (if visible(o.me.x, o.me.y, x, y) then 0 else (0.4 * cell) ^ 2)
            end
            if cost(info.x + dx, info.y + dy) >= cost(info.x - dx, info.y - dy) then
                dx, dy = -dx, -dy
            end
            moveTo(info.x + dx, info.y + dy, true, 0.1 * cell)
        end
        local a = bearing(info.u, info.v)
        rotate(if abs(errorTo(o.me.heading, a)) <= 64 then a else a + 128)
    else
        follow(path, nil)
    end
    retaliate(o)
end

local priority, unlock = 0.01, 0
local lastTick, lastX, lastY, stuck = nil, nil, nil, 0
local function decideGoal(o, elapsed, attackDrive, stalledTicks)
    local me = o.me
    local here = tile(me.x, me.y)
    local dist = distances(here, 12)
    local heat, threat = assess(o, dist)
    local best = priority - (0.001 - 0.0009 * profile.determination) * 1000 * elapsed / o.tick_hz
    local kind, target = nil, nil
    local function offer(name, score, data)
        if score > best then
            kind, best, target = name, score, data
        end
    end
    if threat then
        offer("dodge", 1 - threat.gap / (2.5 * (12 - 8 * profile.boldness) * cell / 10), threat)
    end
    local enemies = {}
    local huntRange = 6 + 14 * profile.cleverness
    for _, t in o.tanks do
        if not t.alive or t.team_id == o.team_id then
            continue
        end
        local id = tile(t.x, t.y)
        local d = dist[id] or math.huge
        table.insert(enemies, id)
        local shootRange = 3 + 5 * profile.cleverness
        if o.ammo > 0 and d < shootRange then
            offer("shoot", 1 - d / (shootRange * (1 + 1.5 * attackDrive)), t)
        end
    end
    if #enemies > 0 and o.ammo == 0 then
        local bias = 1 - 0.8 * profile.boldness
        local limit = #enemies * huntRange * (1 + bias)
        local sum = 0
        for _, id in enemies do
            if not dist[id] then
                dist = distances(here, math.ceil(limit))
            end
            sum += dist[id] or math.huge
        end
        offer("retreat", 1 - sum / limit)
    end
    if stalledTicks > 0 then
        offer("unstick", min(1, stalledTicks / (0.1 * o.tick_hz)))
    end
    for _, id in enemies do
        offer("hunt", 0.2 * (1 - (dist[id] or math.huge) / huntRange), id)
    end
    offer("idle", 0.01)
    priority = best
    if not kind then
        return
    end -- 保留原动作队列；动作耗尽也不会绕过目标锁。
    actions = {}
    unlock = o.tick
        + math.ceil((if kind == "unstick" then 0.03 else 0.8 - 0.7 * profile.cleverness) * o.tick_hz)
        + 1
    if kind == "shoot" then
        attack(o, target)
    elseif kind == "hunt" then
        follow(route(here, target, heat), target)
    elseif kind == "retreat" then
        follow(escape(here, enemies, heat, dist), nil)
        retaliate(o)
    elseif kind == "dodge" then
        dodge(o, target, here, heat, dist)
    elseif kind == "unstick" then
        local d = (if o.drive == "reverse" then 1 else -1) * 0.25 * cell
        moveTo(me.x + d * math.cos(me.heading * rad), me.y + d * math.sin(me.heading * rad), true, 0.1 * cell)
    else
        local mode = floor(3 * random())
        if mode == 0 then
            enqueue({ kind = "wait", delay = 0.1 + 0.4 * random() })
        elseif mode == 1 then
            rotate(random() * 256)
        else
            -- 排除出生格以及活车两格以内的位置；地图解析器保证场地连通。
            local used = {}
            for _, t in o.tanks do
                if t.alive then
                    local origin = tile(t.x, t.y)
                    used[origin] = true
                    for _, n in edges(origin) do
                        used[n] = true
                        for _, m in edges(n) do
                            used[m] = true
                        end
                    end
                end
            end
            local open = {}
            for id = 0, width * height - 1 do
                charge(1)
                if
                    not used[id]
                    and string.byte(rows[2 * floor(id / width) + 2], 2 * (id % width) + 2) == 46
                then
                    table.insert(open, id)
                end
            end
            if #open > 0 then
                local id = open[1 + floor(random() * #open)]
                follow(route(here, id, heat), id)
            end
        end
    end
    return actions
end
local function execute(o)
    local a = active[head]
    local drive, turn, fire = "stop", "none", false
    if a then
        if a.kind == "turn" or a.kind == "move" then
            local moving = a.kind == "move"
            local angle = if moving then bearing(a.x - o.me.x, a.y - o.me.y) + a.offset else a.angle
            local err = errorTo(o.me.heading, angle) * rad
            local reverse = moving and a.reverse and abs(err) > math.pi / 2
            if reverse then
                err = errorTo(o.me.heading, angle + 128) * rad
            end
            if abs(err) > 0.1 then
                turn = if err < 0 then "left" else "right"
            end
            if moving then
                local gap = length(a.x - o.me.x, a.y - o.me.y)
                if gap > 0.1 * cell and abs(err) < 1.13 then
                    drive = if reverse then "reverse" else "forward"
                end
                if gap < a.close then
                    head += 1
                end
            elseif abs(err) < 0.1 then
                head += 1
            end
        else
            a.due = a.due or (o.tick + math.ceil(a.delay * o.tick_hz))
            if o.tick >= a.due then
                if a.heldAt then
                    -- 官网先采样按键再移除 1ms 的 FIRE 动作；60Hz 时连续两帧按住，只触发一次上升沿。
                    fire = o.tick == a.heldAt + 1
                    head += 1
                elseif a.kind == "fire" and o.can_fire then
                    -- 多队适配：不穿过已挡在射线上的队友；并非要求预测必定命中才按键。
                    local _, _, unsafe = firingLine(o, o.me.heading, false)
                    fire = not unsafe
                    if fire then
                        a.heldAt = o.tick
                    else
                        head += 1
                    end
                else
                    head += 1
                end
            end
        end
    end
    return { drive = drive, turn = turn, fire = fire }
end
local planner
local function snapshot(o)
    -- 宿主会原地更新 obs 的行。挂起的规划不能混用两个时刻的弹道和车位。
    local s = table.clone(o)
    s.tanks, s.shells = {}, {}
    for _, t in o.tanks do
        local row = table.clone(t)
        table.insert(s.tanks, row)
        if row.slot_id == o.slot_id then
            s.me = row
        end
    end
    for _, shell in o.shells do
        table.insert(s.shells, table.clone(shell))
    end
    return s
end
return function(o)
    if not rows then
        rows = string.split(o.maze, "\n")
        width, height, cell, wallHalf = floor(#rows[1] / 2), floor(#rows / 2), o.cell, o.wall_half
        seed = (o.behavior_seed or o.slot_id) % 2147483646 + 1
    end
    local elapsed = if lastTick then o.tick - lastTick else 1
    if lastTick and o.drive ~= "stop" and length(o.me.x - lastX, o.me.y - lastY) < o.reverse_speed * 0.2 then
        stuck += elapsed
    else
        stuck = 0
    end
    aggression = min(profile.aggressiveness, aggression + 0.3 * profile.aggressiveness * elapsed / o.tick_hz)
    -- 跳过的观察用真实 tick 差推进时钟，而不是虚构额外决策或补发炮弹。
    if not planner and o.tick >= unlock then
        local s, dt, mood, stalledTicks = snapshot(o), min(elapsed, o.tick - unlock + 1), aggression, stuck
        planner = coroutine.create(function()
            return decideGoal(s, dt, mood, stalledTicks)
        end)
    end
    if planner then
        fuel = 1600
        local ok, queue = coroutine.resume(planner)
        fuel = nil
        if not ok then
            error(queue, 0)
        end
        if coroutine.status(planner) == "dead" then
            if queue then
                active, head = queue, 1
            end
            planner = nil
        end
    end
    lastTick, lastX, lastY = o.tick, o.me.x, o.me.y
    return execute(o)
end
