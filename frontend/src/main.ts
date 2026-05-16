// Copyright (c) 2026 MikeRust contributors. Licensed under AGPL-3.0-only.

import { mount } from 'svelte'
import App from './App.svelte'
import { themeStore } from '$lib/stores/theme.svelte'
import './app.css'

const target = document.getElementById('app')
if (!target) throw new Error('Root #app element missing from index.html')

// Apply the persisted theme before mounting to avoid a flash of the
// wrong palette.
themeStore.init()

export const app = mount(App, { target })
