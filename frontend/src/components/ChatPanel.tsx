import { useEffect, useRef, useState } from 'react';
import MentionPicker from './MentionPicker';
import { api } from '../lib/api';

// ...existing ChatPanel implementation...
// MentionPicker is scoped to the active session so @ references are relative
// to that session's workspace.
