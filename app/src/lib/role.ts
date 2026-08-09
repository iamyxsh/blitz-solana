export type Role = 'user' | 'validator'

const KEY = 'mb.role'

export const readRole = (): Role | null => {
  const stored = localStorage.getItem(KEY)
  return stored === 'user' || stored === 'validator' ? stored : null
}

export const writeRole = (role: Role | null) =>
  role ? localStorage.setItem(KEY, role) : localStorage.removeItem(KEY)
