PORTNAME=	hocuspanda
DISTVERSIONPREFIX=	v
DISTVERSION=	0.1.0
CATEGORIES=	x11-wm wayland

MAINTAINER=	m.bollot@code-troopers.com
COMMENT=	Wayland desktop: compositor and shell in one process
WWW=		https://github.com/mattboll/hocus-panda

LICENSE=	MIT
LICENSE_FILE=	${WRKSRC}/LICENSE

LIB_DEPENDS=	libdrm.so:graphics/libdrm \
		libfontconfig.so:x11-fonts/fontconfig \
		libinput.so:x11/libinput \
		libseat.so:sysutils/seatd \
		libudev.so:devel/libudev-devd \
		libwayland-server.so:graphics/wayland \
		libxkbcommon.so:x11/libxkbcommon

USES=		cargo gl localbase:ldflags pkgconfig xorg
USE_GITHUB=	yes
GH_ACCOUNT=	mattboll
GH_PROJECT=	hocus-panda
# A commit instead of the release, before the first one is tagged: a
# variable of this port only, which the ports it depends on do not read.
.if defined(HOCUSPANDA_COMMIT)
GH_TAGNAME=	${HOCUSPANDA_COMMIT}
.endif
USE_GL=		egl gbm glesv2
USE_XORG=	pixman

# The desktop, its command-line tool and its settings app; the workspace's
# test tools stay out.
CARGO_INSTALL_PATH=	crates/hocuspanda crates/hocus crates/hocus-settings

OPTIONS_DEFINE=		FOOT XWAYLAND
OPTIONS_DEFAULT=	FOOT XWAYLAND
FOOT_DESC=		foot, the drop-down terminal (Super+Backtick)
XWAYLAND_DESC=		Run X11 applications through Xwayland
FOOT_RUN_DEPENDS=	foot:x11/foot
XWAYLAND_RUN_DEPENDS=	Xwayland:x11-servers/xwayland

post-install:
	${MKDIR} ${STAGEDIR}${PREFIX}/share/wayland-sessions \
		${STAGEDIR}${PREFIX}/etc/pam.d
	${INSTALL_DATA} ${WRKSRC}/packaging/hocuspanda.desktop \
		${STAGEDIR}${PREFIX}/share/wayland-sessions
	${INSTALL_DATA} ${WRKSRC}/packaging/pam/freebsd \
		${STAGEDIR}${PREFIX}/etc/pam.d/hocuspanda.sample

# The crates Cargo.lock names, as distfiles: `make cargo-crates` writes it
# (see packaging/README.md).
.sinclude "${.CURDIR}/Makefile.crates"

.include <bsd.port.mk>
