import { useEffect, useRef, useState } from 'react';
import { api } from '../lib/api';
import MentionPicker from './MentionPicker';

// The active session id is used when opening the mention picker.
// Existing ChatPanel behavior remains unchanged; only the MentionPicker
// receives the active session id so its file tree is session-scoped.
