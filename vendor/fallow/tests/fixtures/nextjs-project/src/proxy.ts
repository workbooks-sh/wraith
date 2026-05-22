export function proxy() {
  return Response.redirect('https://example.com');
}

export const config = {
  matcher: ['/dashboard/:path*'],
};

export const unusedProxyHelper = 'still-dead';
