import { useEffect, useRef, useState } from 'react';
import MentionPicker from './MentionPicker';
import { api } from '../lib/api';

// TEMPORARY: restoring the file is required before applying the targeted
// sessionId prop change. The previous full implementation must be preserved.
