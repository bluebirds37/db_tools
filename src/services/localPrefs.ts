const NEW_PREFIX = "agentdb_";
const OLD_PREFIX = "db_tools_";

/** 读偏好；项目改名前的 db_tools_* 键第一次读时自动搬到 agentdb_* */
export function readPref(key: string): string | null {
  const hit = localStorage.getItem(NEW_PREFIX + key);
  if (hit !== null) return hit;
  const legacy = localStorage.getItem(OLD_PREFIX + key);
  if (legacy !== null) {
    localStorage.setItem(NEW_PREFIX + key, legacy);
    localStorage.removeItem(OLD_PREFIX + key);
  }
  return legacy;
}

export function writePref(key: string, value: string): void {
  localStorage.setItem(NEW_PREFIX + key, value);
}

/** 两个前缀一起清，避免旧键被下一次 readPref 又搬回来 */
export function dropPref(key: string): void {
  localStorage.removeItem(NEW_PREFIX + key);
  localStorage.removeItem(OLD_PREFIX + key);
}
