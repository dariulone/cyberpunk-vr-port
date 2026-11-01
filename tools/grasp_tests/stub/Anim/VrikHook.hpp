#pragma once
void VRIK_QuatConj(const float* q, float* o);
void VRIK_QuatMul(const float* a, const float* b, float* o);
void VRIK_QuatNorm(float* q);
void VRIK_QuatRotateVec(const float* q, const float* v, float* o);
