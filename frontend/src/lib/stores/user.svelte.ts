// Copyright (c) 2026 MikeRust contributors. Licensed under AGPL-3.0-only.

import { userApi } from '$lib/api/user'
import { DEFAULT_DOMAIN, type Domain } from '$lib/types/domain'
import { DEFAULT_LOCALE, type Locale, type UserProfile } from '$lib/types/user'

/**
 * User profile + preferences (locale, default domain). Hydrated after a
 * successful unlock; the auth store owns the session identity, this one
 * owns everything persisted in `user_settings`.
 */
function createUserStore() {
  let profile = $state<UserProfile | null>(null)
  let locale = $state<Locale>(DEFAULT_LOCALE)
  let defaultDomain = $state<Domain>(DEFAULT_DOMAIN)
  let loading = $state<boolean>(false)

  return {
    get profile() {
      return profile
    },
    get locale() {
      return locale
    },
    get defaultDomain() {
      return defaultDomain
    },
    get loading() {
      return loading
    },

    /** Pull profile + preferences in one shot (post-unlock). */
    async hydrate() {
      loading = true
      try {
        const [prof, loc, dom] = await Promise.all([
          userApi.getProfile(),
          userApi.getLocale(),
          userApi.getDefaultDomain(),
        ])
        profile = prof
        locale = loc.locale ?? DEFAULT_LOCALE
        defaultDomain = dom.default_domain ?? DEFAULT_DOMAIN
      } finally {
        loading = false
      }
    },

    async setLocale(next: Locale) {
      const res = await userApi.updateLocale(next)
      locale = res.locale
    },

    async setDefaultDomain(next: Domain) {
      const res = await userApi.updateDefaultDomain(next)
      defaultDomain = res.default_domain
    },

    async setDisplayName(name: string | null) {
      await userApi.updateProfile(name)
      if (profile) profile = { ...profile, display_name: name }
    },

    reset() {
      profile = null
      locale = DEFAULT_LOCALE
      defaultDomain = DEFAULT_DOMAIN
    },
  }
}

export const userStore = createUserStore()
