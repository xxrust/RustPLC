import { z } from 'zod';

export const cylinderSchema = z.object({
  label: z.string().min(1, 'Label is required'),
  response_time: z.number().positive('Response time must be positive'),
  status: z.enum(['retracted', 'extended', 'moving', 'fault']).optional(),
});

export const sensorSchema = z.object({
  label: z.string().min(1, 'Label is required'),
  status: z.enum(['on', 'off', 'fault']).optional(),
  value: z.boolean().optional(),
  detects: z.string().optional(),
});

export const switchSchema = z.object({
  label: z.string().min(1, 'Label is required'),
  status: z.enum(['open', 'closed', 'fault']).optional(),
  value: z.boolean().optional(),
});

export const stepperSchema = z.object({
  label: z.string().min(1, 'Label is required'),
  direction: z.enum(['forward', 'reverse', 'stopped']).optional(),
  enable: z.boolean().optional(),
  position: z.number().min(0, 'Position must be non-negative').optional(),
  steps_per_rev: z.number().positive('Steps per revolution must be positive').optional(),
});

export const genericSchema = z.object({
  label: z.string().min(1, 'Label is required'),
});

export type CylinderData = z.infer<typeof cylinderSchema>;
export type SensorData = z.infer<typeof sensorSchema>;
export type SwitchData = z.infer<typeof switchSchema>;
export type StepperData = z.infer<typeof stepperSchema>;
export type GenericData = z.infer<typeof genericSchema>;
