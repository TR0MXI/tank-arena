--!strict
export type Tank = {
    read slot_id: number,
    read team_id: number,
    read alive: boolean,
    x: number,
    y: number,
    heading: number,
}
export type Shot = {
    owner: number,
    id: number,
    x: number,
    y: number,
    vx: number,
    vy: number,
    life: number,
    deadly_to_owner: boolean,
}
export type Obs = {
    read me: Tank,
    read behavior_seed: number?,
    tanks: { Tank },
    shells: { Shot },
    tick: number,
    slot_id: number,
    maze: string,
    ammo: number,
    drive: string,
    turn: string,
    fire_down: boolean,
    can_fire: boolean,
    enemy_ahead: boolean,
    cell: number,
    wall_half: number,
    hull_half_length: number,
    hull_half_width: number,
    shell_radius: number,
    forward_speed: number,
    reverse_speed: number,
    shell_speed: number,
    turn_step: number,
    ammo_limit: number,
    muzzle: number,
    turret_offset: number,
    turret_half_length: number,
    turret_half_width: number,
    hit_radius: number,
}
export type Track = {
    x: number,
    y: number,
    vx: number,
    vy: number,
    at: number,
    loaded: number,
    previousLoad: number?,
    firedAt: number?,
    refill: number,
    covered: boolean,
    coastBudget: number,
    heading: number,
    coast: number?,
}
export type Guard = { t: Tank, room: number, range: number }
export type Field = {
    goal: number,
    dist: { number },
    q: { number },
    head: number,
    blocked: { boolean },
    retargetAt: number?,
}
export type Path = { [number]: number, base: number, x0: number, y0: number, armedAt: number, guess: boolean? }
export type Obstacle = {
    x: number,
    y: number,
    c: number,
    s: number,
    vx: number,
    vy: number,
    coast: number,
    heading: number,
}
export type Paths = {
    [number]: Path,
    steps: number,
    detailed: boolean,
    blockers: { Obstacle },
    memo: { [number]: { number } },
    guards: { Guard }?,
    poseSlice: number?,
}

return {}
