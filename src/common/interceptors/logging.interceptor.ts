import { Injectable, NestInterceptor, ExecutionContext, CallHandler, Logger } from '@nestjs/common';
import { Observable } from 'rxjs';
import { tap, catchError } from 'rxjs/operators';
import { throwError } from 'rxjs';

@Injectable()
export class LoggingInterceptor implements NestInterceptor {
  private readonly logger = new Logger(LoggingInterceptor.name);

  intercept(context: ExecutionContext, next: CallHandler): Observable<any> {
    const request = context.switchToHttp().getRequest();
    const method = request.method;
    const url = request.url;
    const now = Date.now();

    this.logger.log(`Incoming request: ${method} ${url}`);

    return next.handle().pipe(
      tap(() => {
        const responseTime = Date.now() - now;
        this.logger.log(`Request completed: ${method} ${url} - ${responseTime}ms`);
      }),
      catchError((error) => {
        const responseTime = Date.now() - now;
        this.logger.error(
          `Request failed: ${method} ${url} - ${responseTime}ms - ${error.message}`,
          error.stack,
        );
        return throwError(() => error);
      }),
    );
  }
}
