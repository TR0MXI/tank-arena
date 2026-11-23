--!strict
local Frame = require("./frame")
local Types = require("./types")

type Obs = Types.Obs
type Obstacle = Types.Obstacle
type Tank = Types.Tank

local dirs = { { 1, 0 }, { 0, 1 }, { -1, 0 }, { 0, -1 } }
@native
local function cell(x: number, y: number)
    return math.floor(y / Frame.cell) * Frame.width + math.floor(x / Frame.cell) + 1
end

local function center(id: number)
    return ((id - 1) % Frame.width + 0.5) * Frame.cell, (math.floor((id - 1) / Frame.width) + 0.5) * Frame.cell
end

@native
local function wall(vertical: boolean, k: number, row: number)
    if vertical then
        return k <= 0
            or k >= Frame.width
            or (row >= 0 and row < Frame.height and string.byte(Frame.lines[2 * row + 2], 2 * k + 1) == 124)
    end
    return k <= 0
        or k >= Frame.height
        or (row >= 0 and row < Frame.width and string.byte(Frame.lines[2 * k + 1], 2 * row + 2) == 45)
end

local function neighbor(id: number, d: number): number?
    local x, y = (id - 1) % Frame.width, math.floor((id - 1) / Frame.width)
    local dx, dy = dirs[d][1], dirs[d][2]
    if dx ~= 0 then
        if wall(true, x + math.max(dx, 0), y) then
            return nil
        end
    elseif wall(false, y + math.max(dy, 0), x) then
        return nil
    end
    return id + dx + dy * Frame.width
end

-- 扫过游戏薄墙的膨胀矩形，包含墙端。只访问线段包围盒覆盖的格线。
@native
local function slab(
    x: number,
    y: number,
    dx: number,
    dy: number,
    l: number,
    b: number,
    r: number,
    t: number
): (number?, number, number)
    local ax, bx, ay, by = -math.huge, math.huge, -math.huge, math.huge
    if dx == 0 then
        if x <= l or x >= r then
            return nil, 0, 0
        end
    else
        ax, bx = (l - x) / dx, (r - x) / dx
        if ax > bx then
            ax, bx = bx, ax
        end
    end
    if dy == 0 then
        if y <= b or y >= t then
            return nil, 0, 0
        end
    else
        ay, by = (b - y) / dy, (t - y) / dy
        if ay > by then
            ay, by = by, ay
        end
    end
    local entry = math.max(ax, ay)
    if entry > 1 or math.min(bx, by) <= 0 or entry > math.min(bx, by) then
        return nil, 0, 0
    end
    return math.max(0, entry), if ax >= ay then 1 else 0, if ay >= ax then 1 else 0
end

-- 扩张方框只用于粗筛；圆弹擦过墙端时绕圆角法线反射，不能简单翻转 x/y。
@native
local function roundContact(
    x: number,
    y: number,
    dx: number,
    dy: number,
    r: number,
    l: number,
    b: number,
    u: number,
    t: number,
    hit: number
): (number?, number, number)
    local px, py = x + dx * hit, y + dy * hit
    local cx, cy = math.clamp(px, l, u), math.clamp(py, b, t)
    local nx, ny = px - cx, py - cy
    if nx ~= 0 and ny ~= 0 and nx * nx + ny * ny > r * r then
        local ax, ay, speed2 = x - cx, y - cy, dx * dx + dy * dy
        local dot = ax * dx + ay * dy
        local disc = dot * dot - speed2 * (ax * ax + ay * ay - r * r)
        if dot >= 0 or disc <= 0 then
            return nil, 0, 0
        end
        hit = (-dot - math.sqrt(disc)) / speed2
        if hit < 0 or hit > 1 then
            return nil, 0, 0
        end
        nx, ny = ax + hit * dx, ay + hit * dy
    end
    local len = math.sqrt(nx * nx + ny * ny)
    if len == 0 or nx * dx + ny * dy >= 0 then
        return nil, 0, 0
    end
    return hit, nx / len, ny / len
end

@native
local function cast(
    x: number,
    y: number,
    dx: number,
    dy: number,
    r: number,
    rounded: boolean?
): (number?, number, number)
    local lx, hx = math.min(x, x + dx), math.max(x, x + dx)
    local ly, hy = math.min(y, y + dy), math.max(y, y + dy)
    local pad = r + Frame.wall
    local best, flipX, flipY = 2, 0, 0
    for axis = 1, 2 do
        local v = axis == 1
        local lo, hi, otherLo, otherHi, n, m = lx, hx, ly, hy, Frame.width, Frame.height
        if not v then
            lo, hi, otherLo, otherHi, n, m = ly, hy, lx, hx, Frame.height, Frame.width
        end
        for k = math.max(0, math.ceil((lo - pad - Frame.wall) / Frame.cell)), math.min(n, math.floor((hi + pad + Frame.wall) / Frame.cell)) do
            local mid = k * Frame.cell
            for j = math.max(0, math.floor((otherLo - pad) / Frame.cell)), math.min(m - 1, math.floor((otherHi + pad) / Frame.cell)) do
                if not wall(v, k, j) then
                    continue
                end
                local a, b = j * Frame.cell - Frame.wall - r, (j + 1) * Frame.cell + Frame.wall + r
                local l, u = mid - pad, mid + pad
                local hit, fx, fy
                if v then
                    hit, fx, fy = slab(x, y, dx, dy, l, a, u, b)
                else
                    hit, fx, fy = slab(x, y, dx, dy, a, l, b, u)
                end
                if rounded and hit and hit <= best then
                    if v then
                        hit, fx, fy = roundContact(x, y, dx, dy, r, l + r, a + r, u - r, b - r, hit)
                    else
                        hit, fx, fy = roundContact(x, y, dx, dy, r, a + r, l + r, b - r, u - r, hit)
                    end
                end
                if hit and hit <= best then
                    if hit == best then
                        if rounded then
                            local nx, ny = flipX + fx, flipY + fy
                            local length = math.sqrt(nx * nx + ny * ny)
                            if length > 0 then
                                flipX, flipY = nx / length, ny / length
                            end
                        else
                            flipX, flipY = math.max(flipX, fx), math.max(flipY, fy)
                        end
                    else
                        best, flipX, flipY = hit, fx, fy
                    end
                end
            end
        end
    end
    if best <= 1 then
        return best, flipX, flipY
    end
    return nil, 0, 0
end

@native
local function direct(x: number, y: number, tx: number, ty: number, r: number)
    return not cast(x, y, tx - x, ty - y, r)
end

@native
local function vector(h: number, speed: number)
    local r = h * math.pi / 128
    return math.cos(r) * speed, math.sin(r) * speed
end

@native
local function errorTo(me: Tank, x: number, y: number)
    return (math.atan2(y - me.y, x - me.x) * 128 / math.pi - me.heading + 128) % 256 - 128
end

local function turnFor(err: number, slack: number)
    return if err > slack then "right" elseif err < -slack then "left" else "none"
end

-- 只缓存局部格边；大图不一次扫描，8 席 VM 不共享状态。
local wallCache: { { { number } } }, microWalls: { { { number } } }, cacheSize = {}, {}, 0
@native
local function wallsAt(x: number, y: number)
    local id = cell(x, y)
    if wallCache[id] then
        return wallCache[id]
    end
    if cacheSize >= 48 then
        wallCache, microWalls, cacheSize = {}, {}, 0
    end
    local cx, cy = math.floor(x / Frame.cell), math.floor(y / Frame.cell)
    local result = {}
    for side = 0, 1 do
        for n = -1, 1 do
            local j, k = cy + n, cx + side
            if j >= 0 and j < Frame.height and wall(true, k, j) then
                table.insert(result, { k * Frame.cell, (j + 0.5) * Frame.cell, Frame.wall, Frame.cell / 2 + Frame.wall })
            end
            j, k = cx + n, cy + side
            if j >= 0 and j < Frame.width and wall(false, k, j) then
                table.insert(result, { (j + 0.5) * Frame.cell, k * Frame.cell, Frame.cell / 2 + Frame.wall, Frame.wall })
            end
        end
    end
    wallCache[id], cacheSize = result, cacheSize + 1
    return result
end

@native
local function fits(o: Obs, x: number, y: number, c: number, s: number): boolean
    local ix, iy = math.floor(x * 4 / Frame.cell), math.floor(y * 4 / Frame.cell)
    local key = iy * Frame.width * 4 + ix
    local near = microWalls[key]
    if not near then
        near = {}
        local mx, my = (ix + 0.5) * Frame.cell / 4, (iy + 0.5) * Frame.cell / 4
        local bound = o.turret_offset + o.turret_half_length + o.turret_half_width + Frame.cell / 8
        for _, w in wallsAt(x, y) do
            if math.abs(w[1] - mx) < bound + w[3] and math.abs(w[2] - my) < bound + w[4] then
                table.insert(near, w)
            end
        end
        microWalls[key] = near
    end
    local ac, as = math.abs(c), math.abs(s)
    for _, w in near do
        local dx, dy = w[1] - x, w[2] - y
        if
            (math.abs(dx) < w[3] + o.hull_half_length * ac + Frame.hull * as
                and math.abs(dy) < w[4] + o.hull_half_length * as + Frame.hull * ac
                and math.abs(dx * c + dy * s) < o.hull_half_length + w[3] * ac + w[4] * as
                and math.abs(dy * c - dx * s) < Frame.hull + w[3] * as + w[4] * ac)
            or (math.abs((dx - o.turret_offset * c)) < w[3] + o.turret_half_length * ac + o.turret_half_width * as
                and math.abs((dy - o.turret_offset * s)) < w[4] + o.turret_half_length * as + o.turret_half_width * ac
                and math.abs((dx - o.turret_offset * c) * c + (dy - o.turret_offset * s) * s) < o.turret_half_length + w[3] * ac + w[4] * as
                and math.abs((dy - o.turret_offset * s) * c - (dx - o.turret_offset * c) * s) < o.turret_half_width + w[3] * as + w[4] * ac)
        then
            return false
        end
    end
    return true
end

-- 对邻车先做包围盒粗筛，再用四对车身/炮塔矩形验证整车姿态；不分配临时多边形。
@native
local function carOverlap(
    o: Obs,
    x: number,
    y: number,
    c: number,
    s: number,
    t: Obstacle,
    dt: number
): boolean
    local dx, dy = x - (t.x + t.vx * dt), y - (t.y + t.vy * dt)
    if math.max(math.abs(dx), math.abs(dy)) > Frame.pairReach then
        return false
    end
    local u, v = t.c, t.s
    -- 在邻车的局部坐标系中测试分离轴，底盘与炮塔分别占据矩形。
    local lx, ly = dx * u + dy * v, dy * u - dx * v
    local a, b = c * u + s * v, s * u - c * v
    local aa, ab = math.abs(a), math.abs(b)
    local h, q, w, k = o.hull_half_length, o.turret_half_length, o.turret_half_width, o.turret_offset
    if
        (math.abs(lx) < h + h * aa + Frame.hull * ab
                and math.abs(ly) < Frame.hull + h * ab + Frame.hull * aa
                and math.abs(lx * a + ly * b) < h + h * aa + Frame.hull * ab
                and math.abs(ly * a - lx * b) < Frame.hull + h * ab + Frame.hull * aa) or (math.abs((lx - k)) < q + h * aa + Frame.hull * ab
                and math.abs(ly) < w + h * ab + Frame.hull * aa
                and math.abs((lx - k) * a + ly * b) < h + q * aa + w * ab
                and math.abs(ly * a - (lx - k) * b) < Frame.hull + q * ab + w * aa)
    then
        return true
    end
    lx, ly = lx + k * a, ly + k * b
    return (math.abs(lx) < h + q * aa + w * ab
                and math.abs(ly) < Frame.hull + q * ab + w * aa
                and math.abs(lx * a + ly * b) < q + h * aa + Frame.hull * ab
                and math.abs(ly * a - lx * b) < w + h * ab + Frame.hull * aa) or (math.abs((lx - k)) < q + q * aa + w * ab
                and math.abs(ly) < w + q * ab + w * aa
                and math.abs((lx - k) * a + ly * b) < q + q * aa + w * ab
                and math.abs(ly * a - (lx - k) * b) < w + q * ab + w * aa)
end

return {
    cast = cast,
    cell = cell,
    center = center,
    direct = direct,
    errorTo = errorTo,
    fits = fits,
    neighbor = neighbor,
    slab = slab,
    turnFor = turnFor,
    vector = vector,
    carOverlap = carOverlap,
}
